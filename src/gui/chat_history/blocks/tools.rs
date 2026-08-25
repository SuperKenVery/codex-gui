use std::sync::Arc;

use codex_app_server_protocol::{
    CommandExecutionStatus, DynamicToolCallStatus, McpToolCallStatus, PatchApplyStatus,
    PatchChangeKind, ThreadItem,
};
use gpui::{
    AnyElement, App, IntoElement, ParentElement, SharedString, StyleRefinement, Styled, div,
    prelude::*, transparent_white,
};
use gpui_component::{
    Icon, IconName, Sizable as _, StyledExt as _, accordion::Accordion, h_flex, spinner::Spinner,
    theme::Theme,
};

pub(super) fn render_group(
    tools: &[ToolCallView],
    collapsible: bool,
    active_tail: bool,
    expanded: bool,
    theme: &Theme,
    on_toggle: impl Fn(&mut App) + Send + Sync + 'static,
) -> gpui::Div {
    if tools.is_empty() {
        return div();
    }

    let can_toggle = collapsible && !active_tail;
    let open = !can_toggle || expanded;
    let group_id = format!("tool-group-{}", tools[0].id);
    let title_style = StyleRefinement::default().px_0().py_1();
    // .font_normal()
    // .text_color(theme.muted_foreground);
    let content_style = StyleRefinement::default().px_0().pb_0();

    let accordion = Accordion::new(group_id)
        .bordered(false)
        .xsmall()
        .w_full()
        .min_w_0()
        .bg(theme.muted)
        .rounded_lg()
        .item(|item| {
            item.open(open)
                .disabled(!can_toggle)
                .title(render_summary(tools, theme))
                .title_style(title_style)
                .content_style(content_style)
                .hover(|style| style.bg(theme.muted.opacity(0.35)).rounded(theme.radius))
                .bg(theme.transparent)
                .child(render_list(tools, theme).p_5())
        })
        .when(can_toggle, |accordion| {
            accordion.on_toggle_click(move |_, _, cx| on_toggle(cx))
        });

    div()
        .w_full()
        .min_w_0()
        .overflow_x_hidden()
        .py_2()
        .child(accordion)
}

fn render_summary(tools: &[ToolCallView], theme: &Theme) -> gpui::Div {
    let running = tools
        .iter()
        .filter(|tool| matches!(tool.status, ToolStatus::Running))
        .count();
    let failed = tools
        .iter()
        .filter(|tool| matches!(tool.status, ToolStatus::Failed))
        .count();

    h_flex()
        .min_w_0()
        .items_center()
        .gap_1p5()
        .text_sm()
        .child(if running > 0 {
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
        })
        .when(failed > 0, |summary| {
            summary.child(
                div()
                    .text_xs()
                    .text_color(theme.warning_foreground)
                    .child(format!("{failed} failed")),
            )
        })
}

fn render_list(tools: &[ToolCallView], theme: &Theme) -> gpui::Div {
    div()
        .w_full()
        .min_w_0()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap_1p5()
        .children(tools.iter().map(|tool| render_tool(tool, theme)))
}

fn render_tool(tool: &ToolCallView, theme: &Theme) -> gpui::Div {
    h_flex()
        .max_w_full()
        .min_w_0()
        .flex_shrink(1.)
        .items_center()
        .gap_1p5()
        .rounded(theme.radius)
        .border_3()
        .border_color(theme.border)
        .bg(transparent_white())
        .px_2()
        .py_1()
        .text_sm()
        .child(
            Icon::new(tool.kind.icon())
                .xsmall()
                .flex_none()
                .text_color(theme.muted_foreground),
        )
        .child(
            div()
                .min_w_0()
                .flex_1()
                .truncate()
                .whitespace_nowrap()
                .text_color(theme.foreground)
                .child(tool.content.clone()),
        )
        .child(render_trailing(tool, theme))
}

fn render_trailing(tool: &ToolCallView, theme: &Theme) -> AnyElement {
    h_flex()
        .flex_none()
        .items_center()
        .gap_1()
        .when_some(tool.diff, |trailing, diff| {
            trailing
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.success_foreground)
                        .child(format!("+{}", diff.additions)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.danger_foreground)
                        .child(format!("-{}", diff.deletions)),
                )
        })
        .child(render_status(tool.status, theme))
        .into_any_element()
}

fn render_status(status: ToolStatus, theme: &Theme) -> AnyElement {
    match status {
        ToolStatus::Running => Spinner::new()
            .xsmall()
            .color(theme.warning_foreground)
            .into_any_element(),
        ToolStatus::Succeeded => Icon::new(IconName::Check)
            .xsmall()
            .text_color(theme.success_foreground)
            .into_any_element(),
        ToolStatus::Failed => Icon::new(IconName::CircleX)
            .xsmall()
            .text_color(theme.danger_foreground)
            .into_any_element(),
    }
}

#[derive(Clone, Copy)]
enum ToolKind {
    Command,
    File,
    Mcp,
    Dynamic,
}

impl ToolKind {
    fn icon(self) -> IconName {
        match self {
            Self::Command => IconName::SquareTerminal,
            Self::File => IconName::File,
            Self::Mcp => IconName::Globe,
            Self::Dynamic => IconName::Asterisk,
        }
    }
}

#[derive(Clone, Copy)]
enum ToolStatus {
    Running,
    Succeeded,
    Failed,
}

impl ToolStatus {
    fn done(self) -> bool {
        !matches!(self, Self::Running)
    }
}

#[derive(Clone, Copy)]
struct DiffStats {
    additions: usize,
    deletions: usize,
}

#[derive(Clone)]
pub(in crate::gui::chat_history) struct ToolCallView {
    id: SharedString,
    kind: ToolKind,
    content: SharedString,
    diff: Option<DiffStats>,
    status: ToolStatus,
}

pub(in crate::gui::chat_history) fn tool_call_views(tools: &[&ThreadItem]) -> Arc<[ToolCallView]> {
    tools.iter().map(|tool| ToolCallView::new(tool)).collect()
}

impl ToolCallView {
    fn new(tool: &ThreadItem) -> Self {
        let status = tool_status(tool);
        let (kind, content, diff) = tool_call_content(tool, status);
        Self {
            id: tool.id().to_string().into(),
            kind,
            content: content.into(),
            diff,
            status,
        }
    }
}

fn tool_call_content(
    tool: &ThreadItem,
    status: ToolStatus,
) -> (ToolKind, String, Option<DiffStats>) {
    match tool {
        ThreadItem::CommandExecution { command, .. } => {
            let action = if matches!(status, ToolStatus::Running) {
                "Running"
            } else {
                "Ran"
            };
            (
                ToolKind::Command,
                format!("{action} {}", single_line(command)),
                None,
            )
        }
        ThreadItem::FileChange { changes, .. } => {
            if changes.is_empty() {
                return (ToolKind::File, "Preparing file edits".into(), None);
            }

            let diff = changes.iter().fold(
                DiffStats {
                    additions: 0,
                    deletions: 0,
                },
                |mut total, change| {
                    let stats = diff_stats(&change.diff);
                    total.additions += stats.additions;
                    total.deletions += stats.deletions;
                    total
                },
            );
            let action = file_change_action(&changes[0].kind, status);
            let all_same_action = changes
                .iter()
                .all(|change| file_change_action(&change.kind, status) == action);
            let content = if let [change] = changes.as_slice() {
                let path = match &change.kind {
                    PatchChangeKind::Update {
                        move_path: Some(move_path),
                    } => format!("{} → {}", change.path, move_path.display()),
                    _ => change.path.clone(),
                };
                format!("{action} {path}")
            } else if all_same_action {
                format!("{action} {} files", changes.len())
            } else {
                let action = if matches!(status, ToolStatus::Running) {
                    "Changing"
                } else {
                    "Changed"
                };
                format!("{action} {} files", changes.len())
            };
            let diff = (diff.additions > 0 || diff.deletions > 0).then_some(diff);
            (ToolKind::File, content, diff)
        }
        ThreadItem::McpToolCall { server, tool, .. } => {
            let action = if matches!(status, ToolStatus::Running) {
                "Calling"
            } else {
                "Called"
            };
            (ToolKind::Mcp, format!("{action} {server}.{tool}"), None)
        }
        ThreadItem::DynamicToolCall {
            namespace, tool, ..
        } => {
            let name = namespace
                .as_ref()
                .map(|namespace| format!("{namespace}.{tool}"))
                .unwrap_or_else(|| tool.clone());
            let action = if matches!(status, ToolStatus::Running) {
                "Calling"
            } else {
                "Called"
            };
            (ToolKind::Dynamic, format!("{action} {name}"), None)
        }
        _ => (ToolKind::Dynamic, "Tool call".into(), None),
    }
}

fn single_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
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
    !tools.is_empty() && tools.iter().all(|tool| tool_status(tool).done())
}

fn tool_status(item: &ThreadItem) -> ToolStatus {
    match item {
        ThreadItem::CommandExecution { status, .. } => match status {
            CommandExecutionStatus::InProgress => ToolStatus::Running,
            CommandExecutionStatus::Completed => ToolStatus::Succeeded,
            CommandExecutionStatus::Failed | CommandExecutionStatus::Declined => ToolStatus::Failed,
        },
        ThreadItem::FileChange { status, .. } => match status {
            PatchApplyStatus::InProgress => ToolStatus::Running,
            PatchApplyStatus::Completed => ToolStatus::Succeeded,
            PatchApplyStatus::Failed | PatchApplyStatus::Declined => ToolStatus::Failed,
        },
        ThreadItem::McpToolCall { status, .. } => match status {
            McpToolCallStatus::InProgress => ToolStatus::Running,
            McpToolCallStatus::Completed => ToolStatus::Succeeded,
            McpToolCallStatus::Failed => ToolStatus::Failed,
        },
        ThreadItem::DynamicToolCall {
            status, success, ..
        } => match status {
            DynamicToolCallStatus::InProgress => ToolStatus::Running,
            DynamicToolCallStatus::Completed if success != &Some(false) => ToolStatus::Succeeded,
            DynamicToolCallStatus::Completed | DynamicToolCallStatus::Failed => ToolStatus::Failed,
        },
        _ => ToolStatus::Failed,
    }
}

fn file_change_action(kind: &PatchChangeKind, status: ToolStatus) -> &'static str {
    let running = matches!(status, ToolStatus::Running);
    match kind {
        PatchChangeKind::Add if running => "Adding",
        PatchChangeKind::Add => "Added",
        PatchChangeKind::Delete if running => "Deleting",
        PatchChangeKind::Delete => "Deleted",
        PatchChangeKind::Update {
            move_path: None, ..
        } if running => "Editing",
        PatchChangeKind::Update {
            move_path: None, ..
        } => "Edited",
        PatchChangeKind::Update {
            move_path: Some(_), ..
        } if running => "Moving",
        PatchChangeKind::Update {
            move_path: Some(_), ..
        } => "Moved",
    }
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
