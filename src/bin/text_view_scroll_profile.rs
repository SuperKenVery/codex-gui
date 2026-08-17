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
    ScrollDelta, ScrollWheelEvent, Styled as _, Task, Window, WindowBounds, WindowOptions, div,
    point, px, size,
};
use gpui_component::{
    Root, Theme,
    text::{TextView, TextViewState},
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
}

impl Scenario {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "short" => Some(Self::Short),
            "many-blocks" => Some(Self::ManyBlocks),
            "long-list" => Some(Self::LongList),
            "long-code" => Some(Self::LongCode),
            "mixed" => Some(Self::Mixed),
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
                            "unknown scenario {value:?}; expected short, many-blocks, long-list, long-code, or mixed"
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
    "Usage: text_view_scroll_profile [--scenario short|many-blocks|long-list|long-code|mixed] [--seconds N] [--interval-ms N] [--selectable|--not-selectable]"
}

struct ProfileView {
    markdown: gpui::Entity<TextViewState>,
    selectable: bool,
    _scroll_task: Task<()>,
}

impl ProfileView {
    fn new(options: Options, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let markdown = prepared_markdown(options.scenario);
        let markdown_bytes = markdown.len();
        let markdown = cx.new(|cx| TextViewState::markdown(&markdown, cx));
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
                cx.background_executor().timer(interval).await;
            }

            let elapsed = started_at.elapsed();
            let frame_count = frame_count.load(Ordering::Relaxed);
            eprintln!(
                "PROFILE_SCROLL_DONE events={event_count} frames={frame_count} elapsed_ms={} frames_per_second={:.1}",
                elapsed.as_millis(),
                frame_count as f64 / elapsed.as_secs_f64(),
            );
            let _ = cx.update(|_, cx| cx.quit());
        });

        Self {
            markdown,
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
    }
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
    }
}
