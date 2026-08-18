use std::{
    env, process,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
    time::Instant,
};

use gpui::{
    App, AppContext as _, Bounds, Context, IntoElement, ParentElement as _, PlatformInput, Render,
    ScrollDelta, ScrollWheelEvent, SharedString, Styled as _, Task, Window, WindowBounds,
    WindowOptions, div, point, px, size,
};
use gpui_component::{
    ActiveTheme as _, Root, Theme,
    text::{
        MarkdownExtensions, MarkdownNode, MarkdownParseContext, MarkdownPlugin, TextView,
        TextViewState, markdown_ast,
    },
};
use gpui_component_assets::Assets;
use gpui_platform::application;

const DEFAULT_SECONDS: u64 = 15;
// A 60 Hz input cadence measures completed display frames. Pass
// `--interval-ms 4` explicitly for the separate 250 Hz event-flood stress test.
const DEFAULT_INTERVAL_MS: u64 = 16;
const WARMUP: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug)]
enum Scenario {
    Short,
    ManyBlocks,
    LongList,
    LongCode,
    Mixed,
    Transcript,
    TranscriptStreaming,
}

impl Scenario {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "short" => Some(Self::Short),
            "many-blocks" => Some(Self::ManyBlocks),
            "long-list" => Some(Self::LongList),
            "long-code" => Some(Self::LongCode),
            "mixed" => Some(Self::Mixed),
            "transcript" => Some(Self::Transcript),
            "transcript-streaming" => Some(Self::TranscriptStreaming),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Short => "short",
            Self::ManyBlocks => "many-blocks",
            Self::LongList => "long-list",
            Self::LongCode => "long-code",
            Self::Mixed => "mixed",
            Self::Transcript => "transcript",
            Self::TranscriptStreaming => "transcript-streaming",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Options {
    scenario: Scenario,
    selectable: bool,
    seconds: u64,
    interval_ms: u64,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            scenario: Scenario::Mixed,
            selectable: true,
            seconds: DEFAULT_SECONDS,
            interval_ms: DEFAULT_INTERVAL_MS,
        }
    }
}

impl Options {
    fn parse() -> Result<Self, String> {
        let mut options = Self::default();
        let mut args = env::args().skip(1);

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--scenario" => {
                    let value = args.next().ok_or("--scenario requires a value")?;
                    options.scenario = Scenario::parse(&value).ok_or_else(|| {
                        format!(
                            "unknown scenario {value:?}; expected short, many-blocks, long-list, long-code, mixed, transcript, or transcript-streaming"
                        )
                    })?;
                }
                "--seconds" => {
                    options.seconds = parse_positive(&args.next(), "--seconds")?;
                }
                "--interval-ms" => {
                    options.interval_ms = parse_positive(&args.next(), "--interval-ms")?;
                }
                "--selectable" => options.selectable = true,
                "--not-selectable" => options.selectable = false,
                "--help" | "-h" => return Err(usage().into()),
                _ => return Err(format!("unknown argument {arg:?}\n\n{}", usage())),
            }
        }

        Ok(options)
    }
}

fn parse_positive(value: &Option<String>, flag: &str) -> Result<u64, String> {
    let value = value
        .as_deref()
        .ok_or_else(|| format!("{flag} requires a positive integer"))?;
    let parsed = value
        .parse::<u64>()
        .map_err(|_| format!("invalid value for {flag}: {value:?}"))?;
    if parsed == 0 {
        return Err(format!("{flag} must be greater than zero"));
    }
    Ok(parsed)
}

fn usage() -> &'static str {
    "Usage: text_view_scroll_profile [--scenario short|many-blocks|long-list|long-code|mixed|transcript|transcript-streaming] [--seconds N] [--interval-ms N] [--selectable|--not-selectable]"
}

struct ProfileView {
    markdown: gpui::Entity<TextViewState>,
    markdown_extensions: MarkdownExtensions,
    selectable: bool,
    _scroll_task: Task<()>,
}

impl ProfileView {
    fn new(options: Options, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let markdown = prepared_markdown(options.scenario);
        let markdown_bytes = markdown.len();
        let markdown = cx.new(|cx| TextViewState::markdown(&markdown, cx));
        let markdown_for_updates = markdown.clone();
        let markdown_extensions = match options.scenario {
            Scenario::Transcript | Scenario::TranscriptStreaming => {
                MarkdownExtensions::default().plugin(ProfileTranscriptPlugin)
            }
            _ => MarkdownExtensions::default(),
        };
        let scenario = options.scenario.name();

        let scroll_task = window.spawn(cx, async move |cx| {
            cx.background_executor().timer(WARMUP).await;
            eprintln!(
                "PROFILE_SCROLL_START pid={} scenario={scenario} selectable={} bytes={markdown_bytes}",
                process::id(),
                options.selectable,
            );

            let duration = Duration::from_secs(options.seconds);
            let interval = Duration::from_millis(options.interval_ms);
            let started_at = Instant::now();
            let frame_count = Arc::new(AtomicU64::new(0));
            let frame_observer = frame_count.clone();
            if cx
                .update(move |window, _| observe_frames(window, frame_observer))
                .is_err()
            {
                return;
            }
            let mut event_count = 0_u64;
            let mut update_count = 0_u64;

            while started_at.elapsed() < duration {
                // Reverse periodically so the profile keeps exercising layout and paint
                // after reaching either end of the document.
                let direction = if (event_count / 180).is_multiple_of(2) {
                    -1.0
                } else {
                    1.0
                };
                let event = ScrollWheelEvent {
                    position: point(px(500.), px(350.)),
                    delta: ScrollDelta::Pixels(point(px(0.), px(96. * direction))),
                    ..Default::default()
                };

                if cx
                    .update(|window, cx| {
                        window.dispatch_event(PlatformInput::ScrollWheel(event), cx);
                    })
                    .is_err()
                {
                    return;
                }

                event_count += 1;
                if matches!(options.scenario, Scenario::TranscriptStreaming)
                    && event_count.is_multiple_of(60)
                {
                    let delta = transcript_turn(10_000 + event_count as usize);
                    if cx
                        .update(|_, cx| {
                            markdown_for_updates.update(cx, |state, cx| {
                                state.push_str(&delta, cx);
                            });
                        })
                        .is_err()
                    {
                        return;
                    }
                    update_count += 1;
                }
                cx.background_executor().timer(interval).await;
            }

            let elapsed = started_at.elapsed();
            let frame_count = frame_count.load(Ordering::Relaxed);
            eprintln!(
                "PROFILE_SCROLL_DONE events={event_count} updates={update_count} frames={frame_count} elapsed_ms={} frames_per_second={:.1}",
                elapsed.as_millis(),
                frame_count as f64 / elapsed.as_secs_f64(),
            );
            let _ = cx.update(|_, cx| cx.quit());
        });

        Self {
            markdown,
            markdown_extensions,
            selectable: options.selectable,
            _scroll_task: scroll_task,
        }
    }
}

fn observe_frames(window: &Window, frame_count: Arc<AtomicU64>) {
    window.on_next_frame(move |window, _| {
        frame_count.fetch_add(1, Ordering::Relaxed);
        observe_frames(window, frame_count);
    });
}

impl Render for ProfileView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().p_4().child(
            TextView::new(&self.markdown)
                .markdown_extensions(self.markdown_extensions.clone())
                .selectable(self.selectable)
                .scrollable(true)
                .size_full()
                .min_w_0()
                .text_sm()
                .line_height(px(22.)),
        )
    }
}

fn prepared_markdown(scenario: Scenario) -> String {
    match scenario {
        Scenario::Short => {
            "# Short document\n\nA small **Markdown** paragraph.\n\n- one\n- two\n".into()
        }
        Scenario::ManyBlocks => many_blocks_markdown(),
        Scenario::LongList => long_list_markdown(),
        Scenario::LongCode => long_code_markdown(),
        Scenario::Mixed => mixed_markdown(),
        Scenario::Transcript => transcript_markdown(),
        Scenario::TranscriptStreaming => transcript_markdown(),
    }
}

const PROFILE_BLOCK_TAG: &str = "CodexProfileBlock";

fn transcript_markdown() -> String {
    let mut markdown = String::with_capacity(180_000);
    for turn in 0..80 {
        markdown.push_str(&transcript_turn(turn));
    }
    markdown
}

fn transcript_turn(turn: usize) -> String {
    format!(
        r#"<{PROFILE_BLOCK_TAG} kind="user" turn="{turn}" />

<{PROFILE_BLOCK_TAG} kind="assistant" turn="{turn}" />

I inspected the rendering path for turn **{turn}**. This response mixes prose, `inline code`, and enough words to wrap like a normal Codex conversation.

- The first finding describes layout work for turn {turn}.
- The second finding describes paint work and selection state.

<{PROFILE_BLOCK_TAG} kind="tools" turn="{turn}" />

The tool output is complete, and this closing paragraph keeps Markdown adjacent to the custom blocks.

"#
    )
}

#[derive(Clone)]
struct ProfileTranscriptPlugin;

#[derive(Clone)]
struct ProfileBlock {
    kind: SharedString,
    turn: usize,
}

impl MarkdownPlugin for ProfileTranscriptPlugin {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "codex-profile-transcript-block"
    }

    fn parse(
        &self,
        node: &markdown_ast::Node,
        cx: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        let markdown_ast::Node::Html(raw) = node else {
            return None;
        };
        let source = raw.value.trim();
        if !source.starts_with(&format!("<{PROFILE_BLOCK_TAG} ")) {
            return None;
        }
        let kind = html_attr(source, "kind")?;
        let turn = html_attr(source, "turn")?.parse().ok()?;
        Some(
            MarkdownNode::new(
                "codex-profile-transcript-block",
                ProfileBlock {
                    kind: kind.into(),
                    turn,
                },
            )
            .markdown(cx.node_source(node).unwrap_or(source)),
        )
    }

    fn render(&self, node: &MarkdownNode, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(block) = node.data::<ProfileBlock>() else {
            return div().into_any_element();
        };

        match block.kind.as_ref() {
            "user" => render_profile_user(block.turn, cx).into_any_element(),
            "assistant" => div()
                .w_full()
                .min_w_0()
                .pt_2()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child("Codex")
                .into_any_element(),
            "tools" => render_profile_tools(block.turn, cx).into_any_element(),
            _ => div().into_any_element(),
        }
    }
}

fn html_attr(value: &str, name: &str) -> Option<String> {
    let pattern = format!(r#"{name}=""#);
    let start = value.find(&pattern)? + pattern.len();
    let end = value[start..].find('"')?;
    Some(value[start..start + end].to_string())
}

fn render_profile_user(turn: usize, cx: &App) -> gpui::Div {
    div()
        .w_full()
        .min_w_0()
        .overflow_x_hidden()
        .py_2()
        .flex()
        .justify_end()
        .child(
            div()
                .max_w(px(620.))
                .min_w_0()
                .overflow_x_hidden()
                .rounded_lg()
                .bg(cx.theme().secondary)
                .px_3()
                .py_2()
                .text_sm()
                .line_height(px(22.))
                .text_color(cx.theme().secondary_foreground)
                .whitespace_normal()
                .child(format!(
                    "Please investigate the scrolling performance for conversation turn {turn}, including tool calls and user messages."
                )),
        )
}

fn render_profile_tools(turn: usize, cx: &App) -> gpui::Div {
    let tool = |title: &'static str, detail: String| {
        div()
            .w_full()
            .min_w_0()
            .overflow_x_hidden()
            .py_1()
            .flex()
            .items_start()
            .gap_3()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .min_w_0()
                            .overflow_x_hidden()
                            .text_sm()
                            .text_color(cx.theme().foreground)
                            .whitespace_normal()
                            .child(title),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .overflow_x_hidden()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .whitespace_normal()
                            .child(detail),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .text_color(cx.theme().success_foreground)
                    .text_xs()
                    .child("done"),
            )
    };

    let tools = (0..3).fold(
        div().w_full().min_w_0().flex().flex_col().gap_2(),
        |tools, tool_index| {
            let (title, detail) = match tool_index % 3 {
                0 => (
                    "Terminal",
                    format!("rg -n render src/gui (turn {turn}, call {tool_index})"),
                ),
                1 => (
                    "File edit",
                    format!(
                        "edited src/gui/chat_history.rs (+12 -4), turn {turn}, call {tool_index}"
                    ),
                ),
                _ => (
                    "MCP tool",
                    format!("browser.inspect_visible_state turn-{turn}-call-{tool_index}"),
                ),
            };
            tools.child(tool(title, detail))
        },
    );

    div()
        .w_full()
        .min_w_0()
        .overflow_x_hidden()
        .py_2()
        .child(tools)
}

fn many_blocks_markdown() -> String {
    let mut markdown = String::with_capacity(160_000);
    for index in 0..800 {
        markdown.push_str(&format!(
            "## Independent block {index}\n\nThis is paragraph {index} with **bold text**, `inline code`, and enough words to wrap across the profiling window.\n\n"
        ));
    }
    markdown
}

fn long_list_markdown() -> String {
    let mut markdown = String::from("# One top-level list block\n\n");
    for index in 0..600 {
        markdown.push_str(&format!(
            "- List item {index} has **bold text**, `inline code`, and enough words to exercise shaping and wrapping.\n"
        ));
    }
    markdown
}

fn long_code_markdown() -> String {
    let mut markdown = String::from("# One top-level code block\n\n```rust\n");
    for index in 0..800 {
        markdown.push_str(&format!(
            "let profile_line_{index} = \"TextView scroll profile line {index}\";\n"
        ));
    }
    markdown.push_str("```\n");
    markdown
}

fn mixed_markdown() -> String {
    let mut markdown = String::with_capacity(160_000);
    for turn in 0..60 {
        markdown.push_str(&format!(
            "# Turn {turn}\n\nThis response contains **formatted prose**, `inline code`, and a [link](https://example.com/{turn}). It is deliberately deterministic so repeated profiles are comparable.\n\n"
        ));
        for item in 0..8 {
            markdown.push_str(&format!(
                "- Turn {turn}, item {item}: a moderately long list row that wraps in a normal chat window.\n"
            ));
        }
        markdown.push_str("\n```rust\nfn profile_fixture() {\n");
        for line in 0..6 {
            markdown.push_str(&format!("    let line_{line} = {turn} + {line};\n"));
        }
        markdown.push_str("}\n```\n\n> A short blockquote closes this synthetic turn.\n\n---\n\n");
    }
    markdown
}

fn run(options: Options) {
    application().with_assets(Assets).run(move |cx: &mut App| {
        gpui_component::init(cx);
        Theme::sync_system_appearance(None, cx);

        let bounds = Bounds::centered(None, size(px(1000.), px(700.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            move |window, cx| {
                window.set_window_title("TextView scroll profile");
                let view = cx.new(|cx| ProfileView::new(options, window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            },
        )
        .expect("open TextView profile window");
        cx.activate(true);
    });
}

fn main() {
    match Options::parse() {
        Ok(options) => run(options),
        Err(message) => {
            eprintln!("{message}");
            process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Entity, TestAppContext, VisualTestContext};

    #[derive(Clone)]
    struct CountingPlugin(Arc<AtomicU64>);

    impl MarkdownPlugin for CountingPlugin {
        fn is_block(&self) -> bool {
            true
        }

        fn name(&self) -> &str {
            "counting-profile-block"
        }

        fn parse(
            &self,
            node: &markdown_ast::Node,
            cx: &MarkdownParseContext<'_>,
        ) -> Option<MarkdownNode> {
            let markdown_ast::Node::Html(raw) = node else {
                return None;
            };
            (raw.value.trim() == "<CountProfileBlock />").then(|| {
                MarkdownNode::new("counting-profile-block", ())
                    .markdown(cx.node_source(node).unwrap_or(raw.value.as_str()))
            })
        }

        fn render(
            &self,
            _node: &MarkdownNode,
            _window: &mut Window,
            _cx: &mut App,
        ) -> impl IntoElement {
            self.0.fetch_add(1, Ordering::Relaxed);
            div().h(px(24.)).child("custom block")
        }
    }

    struct CountingRoot {
        state: Entity<TextViewState>,
        extensions: MarkdownExtensions,
    }

    impl Render for CountingRoot {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().w(px(500.)).h(px(400.)).child(
                TextView::new(&self.state)
                    .markdown_extensions(self.extensions.clone())
                    .scrollable(true)
                    .size_full(),
            )
        }
    }

    #[test]
    fn prepared_scenarios_have_the_expected_block_shape() {
        let many_blocks = prepared_markdown(Scenario::ManyBlocks);
        assert!(many_blocks.matches("## Independent block").count() >= 800);

        let long_list = prepared_markdown(Scenario::LongList);
        assert!(
            long_list
                .lines()
                .filter(|line| line.starts_with("- "))
                .count()
                >= 600
        );

        let long_code = prepared_markdown(Scenario::LongCode);
        assert_eq!(long_code.matches("```rust").count(), 1);
        assert_eq!(long_code.matches("\n```\n").count(), 1);

        let transcript = prepared_markdown(Scenario::Transcript);
        assert_eq!(transcript.matches(r#"kind="user""#).count(), 80);
        assert_eq!(transcript.matches(r#"kind="tools""#).count(), 80);
    }

    #[gpui::test]
    fn appending_custom_blocks_keeps_measured_prefix(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let renders = Arc::new(AtomicU64::new(0));
        let initial = "<CountProfileBlock />\n\n".repeat(100);
        let extensions = MarkdownExtensions::default().plugin(CountingPlugin(renders.clone()));
        let (content, cx) = cx.add_window_view(|_, cx| {
            let state = cx.new(|cx| TextViewState::markdown(&initial, cx));
            CountingRoot { state, extensions }
        });
        let cx: &mut VisualTestContext = cx;

        // The first draw installs the plugin; the second lays out the parsed
        // custom blocks and records their exact heights.
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert!(renders.load(Ordering::Relaxed) >= 100);

        renders.store(0, Ordering::Relaxed);
        content.update(cx, |root, cx| {
            root.state.update(cx, |state, cx| {
                state.push_str("<CountProfileBlock />\n\n", cx);
            });
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });

        let render_count = renders.load(Ordering::Relaxed);
        assert!(
            render_count <= 25,
            "append re-rendered the already measured custom-block prefix: {render_count} blocks"
        );
    }
}
