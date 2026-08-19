use std::{sync::Arc, time::Duration};

use codex_app_server_protocol::{
    CommandExecutionStatus, DynamicToolCallStatus, McpToolCallStatus, PatchApplyStatus,
    PatchChangeKind, ThreadItem, UserInput,
};
use gpui::{
    App, ClickEvent, IntoElement, ParentElement, SharedString, Styled, Window, div, prelude::*, px,
};
use gpui_component::{
    Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    theme::Theme,
    v_flex,
};

pub(super) fn chat_tree_item(
    id: impl Into<gpui::ElementId>,
    title: SharedString,
    _subtitle: SharedString,
    selected: bool,
    theme: &Theme,
) -> Button {
    Button::new(id)
        .ghost()
        .with_size(px(0.))
        .w_full()
        .rounded_lg()
        .child(
            v_flex()
                .w_full()
                .min_w_0()
                .gap_0p5()
                .items_start()
                .rounded_lg()
                .py_1p5()
                .pl_7()
                .pr_2()
                .when(selected, |this| this.bg(theme.sidebar_accent.opacity(0.38)))
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

pub(super) fn render_notice(body: &str, theme: &Theme) -> impl IntoElement {
    div()
        .w_full()
        .min_w_0()
        .overflow_x_hidden()
        .py_2()
        .text_sm()
        .text_color(theme.foreground)
        .child(body.to_string())
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

pub(super) fn render_assistant_header(author: &'static str, theme: &Theme) -> gpui::Div {
    div()
        .w_full()
        .min_w_0()
        .pt_2()
        .text_xs()
        .text_color(theme.muted_foreground)
        .child(author)
}

pub(super) fn render_tool_group(
    tools: &[ToolCallView],
    collapse_tools: bool,
    active_tool_tail: bool,
    expanded: bool,
    theme: &Theme,
    on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::Div {
    if tools.is_empty() {
        return div();
    }

    let should_collapse = collapse_tools && !active_tool_tail;
    let tools_view = if should_collapse {
        let mut tool_group = div().flex().flex_col().gap_2().child(
            render_tool_summary(tools, theme, expanded)
                .id(format!("tool-summary-{}", tools[0].id))
                .on_click(on_toggle),
        );
        if expanded {
            tool_group = tool_group.child(render_tool_list(tools, theme));
        }
        tool_group.into_any_element()
    } else {
        render_tool_list(tools, theme).into_any_element()
    };

    div()
        .w_full()
        .min_w_0()
        .overflow_x_hidden()
        .py_2()
        .child(tools_view)
}

pub(super) fn render_user_message(body: SharedString, theme: &Theme) -> gpui::Div {
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
                .child(body),
        )
}

fn render_tool_summary(tools: &[ToolCallView], theme: &Theme, expanded: bool) -> gpui::Div {
    let running = tools.iter().filter(|tool| !tool.done).count();
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

fn render_tool_list(tools: &[ToolCallView], theme: &Theme) -> gpui::Div {
    tools.iter().fold(
        div().w_full().min_w_0().flex().flex_col().gap_2(),
        |list, tool| list.child(render_tool_call(tool, theme)),
    )
}

fn render_tool_call(tool: &ToolCallView, theme: &Theme) -> gpui::Div {
    let (label, color) = if tool.done {
        ("done", theme.success_foreground)
    } else {
        ("running", theme.warning_foreground)
    };
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
                        .child(tool.title.clone()),
                )
                .child(
                    div()
                        .min_w_0()
                        .overflow_x_hidden()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .whitespace_normal()
                        .child(tool.detail.clone()),
                ),
        )
        .child(div().flex_none().text_color(color).text_xs().child(label))
}

#[derive(Clone)]
pub(super) struct ToolCallView {
    id: SharedString,
    title: SharedString,
    detail: SharedString,
    done: bool,
}

pub(super) fn tool_call_views(tools: &[&ThreadItem]) -> Arc<[ToolCallView]> {
    tools.iter().map(|tool| ToolCallView::new(tool)).collect()
}

impl ToolCallView {
    fn new(tool: &ThreadItem) -> Self {
        let (title, detail) = tool_call_text(tool);
        Self {
            id: tool.id().to_string().into(),
            title: title.into(),
            detail: detail.into(),
            done: tool_item_done(tool),
        }
    }
}

fn tool_call_text(tool: &ThreadItem) -> (String, String) {
    match tool {
        ThreadItem::CommandExecution { command, cwd, .. } => (
            "Terminal".into(),
            format!("{command} ({})", cwd.render_for_ui()),
        ),
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

pub(super) fn user_input_text(content: &[UserInput]) -> String {
    content
        .iter()
        .filter_map(|input| match input {
            UserInput::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
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
