use std::time::Duration;

use crate::gui::{MessageState, StreamState};
use codex_app_server_protocol::{
    CommandExecutionStatus, DynamicToolCallStatus, McpToolCallStatus, PatchApplyStatus,
    PatchChangeKind, ThreadItem, UserInput,
};
use codex_protocol::models::MessagePhase;
use gpui::{
    App, ClickEvent, Entity, IntoElement, ParentElement, SharedString, Styled, Window, div,
    prelude::*, px, rems,
};
use gpui_component::{
    Icon, IconName, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    clipboard::Clipboard,
    h_flex,
    text::{TextView, TextViewState, markdown},
    theme::Theme,
    v_flex,
};
use zed_markdown::{
    Markdown as ZedMarkdown, MarkdownElement as ZedMarkdownElement,
    MarkdownStyle as ZedMarkdownStyle,
};

const LARGE_MARKDOWN_BODY_BYTES: usize = 12 * 1024;
const LARGE_MARKDOWN_VIEW_HEIGHT: f32 = 520.;

pub(super) fn chat_tree_item(
    id: impl Into<gpui::ElementId>,
    title: SharedString,
    _subtitle: SharedString,
    selected: bool,
    _theme: &Theme,
) -> Button {
    Button::new(id)
        .ghost()
        .selected(selected)
        .with_size(px(0.))
        .w_full()
        .rounded_lg()
        .child(
            v_flex()
                .w_full()
                .min_w_0()
                .gap_0p5()
                .items_start()
                .py_1p5()
                .pl_7()
                .pr_2()
                .child(
                    div()
                        .w_full()
                        .text_sm()
                        .line_height(px(18.))
                        .overflow_x_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(title),
                ),
        )
}

pub(super) fn status_pill(label: String, theme: &Theme) -> impl IntoElement {
    div()
        .px_2()
        .py_1()
        .rounded_sm()
        .bg(theme.secondary)
        .text_xs()
        .text_color(theme.secondary_foreground)
        .child(label)
}

pub(super) fn render_notice(body: &str, theme: &Theme) -> impl IntoElement {
    notice_message_block(body, None, theme, None)
}

pub(super) fn render_thread_item_state(
    item: Option<&ThreadItem>,
    tools: &[&ThreadItem],
    state: &MessageState,
    collapse_tools: bool,
    hide_tools: bool,
    active_tool_tail: bool,
    theme: &Theme,
    window: &mut Window,
    on_toggle_tools: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    match item {
        Some(ThreadItem::UserMessage { content, .. }) => {
            user_message_block(&user_input_text(content), theme)
        }
        Some(ThreadItem::AgentMessage { text, phase, .. }) => render_assistant_message(
            text,
            message_phase(phase.as_ref()),
            state.stream_state,
            tools,
            state.body_view.as_ref(),
            collapse_tools,
            hide_tools,
            active_tool_tail,
            theme,
            Some((state.tools_expanded, Box::new(on_toggle_tools))),
            Some((state.zed_markdown.as_ref(), window)),
        ),
        Some(item) if is_tool_item(item) => render_assistant_message(
            "",
            AssistantPhase::Commentary,
            state.stream_state,
            tools,
            state.body_view.as_ref(),
            collapse_tools,
            hide_tools,
            active_tool_tail,
            theme,
            Some((state.tools_expanded, Box::new(on_toggle_tools))),
            Some((state.zed_markdown.as_ref(), window)),
        ),
        None => notice_message_block(
            &state.rendered_body,
            state.body_view.as_ref(),
            theme,
            Some((state.zed_markdown.as_ref(), window)),
        ),
        Some(_) => div(),
    }
}

pub(super) fn render_worked_summary(
    duration: Duration,
    theme: &Theme,
    expanded: bool,
) -> gpui::Div {
    div()
        .w_full()
        .min_w_0()
        .py_1()
        .cursor_pointer()
        .text_sm()
        .text_color(theme.muted_foreground)
        .child(
            h_flex()
                .items_center()
                .gap_1()
                .child(disclosure_icon(expanded, theme))
                .child(format!("Worked for {}", format_duration(duration))),
        )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AssistantPhase {
    Commentary,
    FinalAnswer,
}

fn render_assistant_message(
    body: &str,
    phase: AssistantPhase,
    stream_state: StreamState,
    tools: &[&ThreadItem],
    body_view: Option<&Entity<TextViewState>>,
    collapse_tools: bool,
    hide_tools: bool,
    active_tool_tail: bool,
    theme: &Theme,
    tool_toggle: Option<(
        bool,
        Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>,
    )>,
    zed_markdown: Option<(Option<&Entity<ZedMarkdown>>, &mut Window)>,
) -> gpui::Div {
    let mut block = message_block(
        match (phase, stream_state) {
            (AssistantPhase::Commentary, _) => "",
            (AssistantPhase::FinalAnswer, StreamState::Complete) => "Codex",
            (AssistantPhase::FinalAnswer, StreamState::Streaming) => "Codex is working",
        },
        body,
        body_view,
        theme,
        zed_markdown,
    );

    if !hide_tools && !tools.is_empty() {
        let should_collapse = collapse_tools && !active_tool_tail;
        let expanded = tool_toggle
            .as_ref()
            .map(|(expanded, _)| *expanded)
            .unwrap_or(false);
        let tools_view = if should_collapse {
            let mut tool_group = div().flex().flex_col().gap_2();
            let summary = match tool_toggle {
                Some((_, on_toggle)) => render_tool_summary(tools, theme, expanded)
                    .id(format!("tool-summary-{}", tools[0].id()))
                    .on_click(on_toggle)
                    .into_any_element(),
                None => render_tool_summary(tools, theme, expanded).into_any_element(),
            };
            tool_group = tool_group.child(summary);
            if expanded {
                tool_group = tool_group.child(render_tool_list(tools, theme));
            }
            tool_group.into_any_element()
        } else {
            render_tool_list(tools, theme).into_any_element()
        };
        block = block.child(tools_view);
    }

    block
}

fn notice_message_block(
    body: &str,
    body_view: Option<&Entity<TextViewState>>,
    theme: &Theme,
    zed_markdown: Option<(Option<&Entity<ZedMarkdown>>, &mut Window)>,
) -> gpui::Div {
    message_block("", body, body_view, theme, zed_markdown)
}

fn user_message_block(body: &str, theme: &Theme) -> gpui::Div {
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
                .bg(theme.secondary)
                .px_3()
                .py_2()
                .text_sm()
                .line_height(px(22.))
                .text_color(theme.secondary_foreground)
                .whitespace_normal()
                .child(body.to_string()),
        )
}

fn message_block(
    author: &'static str,
    body: &str,
    body_view: Option<&Entity<TextViewState>>,
    theme: &Theme,
    zed_markdown: Option<(Option<&Entity<ZedMarkdown>>, &mut Window)>,
) -> gpui::Div {
    let is_large_body = body.len() >= LARGE_MARKDOWN_BODY_BYTES;
    let body = if body.is_empty() {
        None
    } else if let Some((Some(markdown), window)) = zed_markdown {
        ZedMarkdownElement::new(markdown.clone(), zed_markdown_style(window, theme))
            .into_any_element()
            .into()
    } else {
        Some(
            body_view
                .map(TextView::new)
                .unwrap_or_else(|| markdown(body.to_string()))
                .selectable(true)
                .code_block_actions(|code_block, _, _| {
                    h_flex()
                        .gap_1()
                        .child(Clipboard::new("copy-code").value(code_block.code().clone()))
                })
                .when(is_large_body, |view| {
                    view.h(px(LARGE_MARKDOWN_VIEW_HEIGHT)).scrollable(true)
                })
                .into_any_element(),
        )
    };

    div()
        .w_full()
        .min_w_0()
        .overflow_x_hidden()
        .py_2()
        .flex()
        .flex_col()
        .gap_2()
        .when(!author.is_empty(), |block| {
            block.child(
                div()
                    .min_w_0()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(author),
            )
        })
        .when_some(body, |block, body| {
            block.child(div().w_full().min_w_0().overflow_x_hidden().child(body))
        })
}

fn zed_markdown_style(window: &Window, theme: &Theme) -> ZedMarkdownStyle {
    let mut style = ZedMarkdownStyle::default();
    let mut text_style = window.text_style();
    text_style.refine(&gpui::TextStyleRefinement {
        font_size: Some(rems(0.875).into()),
        line_height: Some(px(22.).into()),
        color: Some(theme.foreground),
        ..Default::default()
    });
    style.base_text_style = text_style;
    style.code_block_overflow_x_scroll = true;
    style
}

fn render_tool_summary(tools: &[&ThreadItem], theme: &Theme, expanded: bool) -> gpui::Div {
    let running = tools.iter().filter(|tool| !tool_item_done(tool)).count();
    let label = if running > 0 {
        format!(
            "Running {} {}",
            tools.len(),
            pluralize(tools.len(), "tool call")
        )
    } else {
        format!(
            "Ran {} {}",
            tools.len(),
            pluralize(tools.len(), "tool call")
        )
    };

    div()
        .w_full()
        .min_w_0()
        .cursor_pointer()
        .text_sm()
        .text_color(theme.muted_foreground)
        .child(
            h_flex()
                .items_center()
                .gap_1()
                .child(disclosure_icon(expanded, theme))
                .child(label),
        )
}

fn disclosure_icon(expanded: bool, theme: &Theme) -> impl IntoElement {
    Icon::new(if expanded {
        IconName::ChevronDown
    } else {
        IconName::ChevronRight
    })
    .xsmall()
    .text_color(theme.muted_foreground)
}

fn render_tool_list(tools: &[&ThreadItem], theme: &Theme) -> gpui::Div {
    tools.iter().fold(
        div().w_full().min_w_0().flex().flex_col().gap_2(),
        |list, tool| list.child(render_tool_call(tool, theme)),
    )
}

fn render_tool_call(tool: &ThreadItem, theme: &Theme) -> gpui::Div {
    let (label, color) = if tool_item_done(tool) {
        ("done", theme.success_foreground)
    } else {
        ("running", theme.warning_foreground)
    };
    let (title, detail) = tool_call_text(tool);

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
                        .text_color(theme.foreground)
                        .whitespace_normal()
                        .child(title),
                )
                .child(
                    div()
                        .min_w_0()
                        .overflow_x_hidden()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .whitespace_normal()
                        .child(detail),
                ),
        )
        .child(div().flex_none().text_color(color).text_xs().child(label))
}

fn tool_call_text(tool: &ThreadItem) -> (String, String) {
    match tool {
        ThreadItem::CommandExecution { command, cwd, .. } => {
            ("Terminal".into(), format!("{command} ({})", cwd.display()))
        }
        ThreadItem::FileChange { changes, .. } => {
            let detail = if changes.is_empty() {
                "Preparing file edits".into()
            } else {
                changes
                    .iter()
                    .map(|change| {
                        let action = file_change_action(&change.kind);
                        let path = match &change.kind {
                            PatchChangeKind::Update {
                                move_path: Some(move_path),
                            } => format!("{} -> {}", change.path, move_path.display()),
                            _ => change.path.clone(),
                        };
                        let stats = diff_stats(&change.diff);
                        format!(
                            "{action} {path} (+{} -{})",
                            stats.additions, stats.deletions
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            ("File edit".into(), detail)
        }
        ThreadItem::McpToolCall { server, tool, .. } => {
            ("MCP tool".into(), format!("{server}.{tool}"))
        }
        ThreadItem::DynamicToolCall {
            namespace, tool, ..
        } => {
            let detail = namespace
                .as_ref()
                .map(|namespace| format!("{namespace}.{tool}"))
                .unwrap_or_else(|| tool.clone());
            ("Tool call".into(), detail)
        }
        _ => ("Tool call".into(), String::new()),
    }
}

fn user_input_text(content: &[UserInput]) -> String {
    content
        .iter()
        .filter_map(|input| match input {
            UserInput::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn message_phase(phase: Option<&MessagePhase>) -> AssistantPhase {
    match phase {
        Some(MessagePhase::Commentary) => AssistantPhase::Commentary,
        Some(MessagePhase::FinalAnswer) | None => AssistantPhase::FinalAnswer,
    }
}

fn is_tool_item(item: &ThreadItem) -> bool {
    matches!(
        item,
        ThreadItem::CommandExecution { .. }
            | ThreadItem::FileChange { .. }
            | ThreadItem::McpToolCall { .. }
            | ThreadItem::DynamicToolCall { .. }
    )
}

fn tool_item_done(item: &ThreadItem) -> bool {
    match item {
        ThreadItem::CommandExecution { status, .. } => {
            !matches!(status, CommandExecutionStatus::InProgress)
        }
        ThreadItem::FileChange { status, .. } => !matches!(status, PatchApplyStatus::InProgress),
        ThreadItem::McpToolCall { status, .. } => !matches!(status, McpToolCallStatus::InProgress),
        ThreadItem::DynamicToolCall { status, .. } => {
            !matches!(status, DynamicToolCallStatus::InProgress)
        }
        _ => false,
    }
}

fn file_change_action(kind: &PatchChangeKind) -> &'static str {
    match kind {
        PatchChangeKind::Add => "added",
        PatchChangeKind::Delete => "deleted",
        PatchChangeKind::Update { move_path: None } => "edited",
        PatchChangeKind::Update { move_path: Some(_) } => "moved",
    }
}

struct DiffStats {
    additions: usize,
    deletions: usize,
}

fn diff_stats(diff: &str) -> DiffStats {
    let mut additions = 0;
    let mut deletions = 0;
    for line in diff.lines() {
        if line.starts_with("+++") || line.starts_with("---") {
            continue;
        }
        if line.starts_with('+') {
            additions += 1;
        } else if line.starts_with('-') {
            deletions += 1;
        }
    }
    DiffStats {
        additions,
        deletions,
    }
}

fn pluralize(count: usize, singular: &'static str) -> &'static str {
    if count == 1 { singular } else { "tool calls" }
}

fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let minutes = seconds / 60;
    let seconds = seconds % 60;

    if minutes > 0 {
        format!("{minutes}m {seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}
