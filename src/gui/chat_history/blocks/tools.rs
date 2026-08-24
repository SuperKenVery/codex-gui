use std::sync::Arc;

use codex_app_server_protocol::{
    CommandExecutionStatus, DynamicToolCallStatus, McpToolCallStatus, PatchApplyStatus,
    PatchChangeKind, ThreadItem,
};
use gpui::{
    App, ClickEvent, IntoElement, ParentElement, SharedString, Styled, Window, div, prelude::*,
};
use gpui_component::{Icon, IconName, Sizable as _, h_flex, theme::Theme};

pub(super) fn render_group(
    tools: &[ToolCallView],
    collapsible: bool,
    active_tail: bool,
    expanded: bool,
    theme: &Theme,
    on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::Div {
    if tools.is_empty() {
        return div();
    }

    let should_collapse = collapsible && !active_tail;
    let tools_view = if should_collapse {
        let mut tool_group = div().flex().flex_col().gap_2().child(
            render_summary(tools, theme, expanded)
                .id(format!("tool-summary-{}", tools[0].id))
                .on_click(on_toggle),
        );
        if expanded {
            tool_group = tool_group.child(render_list(tools, theme));
        }
        tool_group.into_any_element()
    } else {
        render_list(tools, theme).into_any_element()
    };

    div()
        .w_full()
        .min_w_0()
        .overflow_x_hidden()
        .py_2()
        .child(tools_view)
}

fn render_summary(tools: &[ToolCallView], theme: &Theme, expanded: bool) -> gpui::Div {
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

fn render_list(tools: &[ToolCallView], theme: &Theme) -> gpui::Div {
    tools.iter().fold(
        div().w_full().min_w_0().flex().flex_col().gap_2(),
        |list, tool| list.child(render_tool(tool, theme)),
    )
}

fn render_tool(tool: &ToolCallView, theme: &Theme) -> gpui::Div {
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
pub(in crate::gui::chat_history) struct ToolCallView {
    id: SharedString,
    title: SharedString,
    detail: SharedString,
    done: bool,
}

pub(in crate::gui::chat_history) fn tool_call_views(tools: &[&ThreadItem]) -> Arc<[ToolCallView]> {
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

pub(in crate::gui::chat_history) fn is_tool_item(item: &ThreadItem) -> bool {
    matches!(
        item,
        ThreadItem::CommandExecution { .. }
            | ThreadItem::FileChange { .. }
            | ThreadItem::McpToolCall { .. }
            | ThreadItem::DynamicToolCall { .. }
    )
}

pub(in crate::gui::chat_history) fn tools_done(tools: &[&ThreadItem]) -> bool {
    !tools.is_empty() && tools.iter().all(|tool| tool_item_done(tool))
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
