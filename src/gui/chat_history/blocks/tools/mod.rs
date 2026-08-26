mod collaboration;
mod command;
mod file_change;
mod media;
mod remote;
mod simple;
mod sleep;
mod web_search;

use std::{collections::HashMap, sync::Arc};

use codex_app_server_protocol::{
    CollabAgentToolCallStatus, CommandExecutionStatus, DynamicToolCallStatus, McpToolCallStatus,
    PatchApplyStatus, ThreadItem,
};
use gpui::{
    App, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div, prelude::*,
};
use gpui_component::{Sizable as _, accordion::Accordion, h_flex, theme::Theme};

use collaboration::CollaborationTool;
use command::CommandTool;
use file_change::FileChangeTool;
use media::{ImageGenerationTool, ImageViewTool};
use remote::{DynamicTool, McpTool};
use simple::{SimpleTool, SimpleToolElement, ToolStatus};
use sleep::SleepTool;
use web_search::WebSearchTool;

#[derive(Clone, IntoElement)]
pub(in crate::gui::chat_history) enum ToolCall {
    Command(CommandTool),
    FileChange(FileChangeTool),
    Mcp(McpTool),
    Dynamic(DynamicTool),
    WebSearch(WebSearchTool),
    ImageView(ImageViewTool),
    Collaboration(CollaborationTool),
    Sleep(SleepTool),
    ImageGeneration(ImageGenerationTool),
}

impl ToolCall {
    fn new(item: &ThreadItem, progress: Option<&[SharedString]>, streaming: bool) -> Option<Self> {
        let status = tool_status(item, streaming);
        Some(match item {
            ThreadItem::CommandExecution { .. } => {
                Self::Command(CommandTool::new(item, status, progress)?)
            }
            ThreadItem::FileChange { .. } => {
                Self::FileChange(FileChangeTool::new(item, status, progress)?)
            }
            ThreadItem::McpToolCall { .. } => Self::Mcp(McpTool::new(item, status, progress)?),
            ThreadItem::DynamicToolCall { .. } => {
                Self::Dynamic(DynamicTool::new(item, status, progress)?)
            }
            ThreadItem::WebSearch(_) => Self::WebSearch(WebSearchTool::new(item, status)?),
            ThreadItem::ImageView { .. } => Self::ImageView(ImageViewTool::new(item, status)?),
            ThreadItem::CollabAgentToolCall { .. } => {
                Self::Collaboration(CollaborationTool::new(item, status, progress)?)
            }
            ThreadItem::Sleep(_) => Self::Sleep(SleepTool::new(item, status)?),
            ThreadItem::ImageGeneration(_) => {
                Self::ImageGeneration(ImageGenerationTool::new(item, status, progress)?)
            }
            _ => return None,
        })
    }

    fn status(&self) -> ToolStatus {
        match self {
            Self::Command(tool) => tool.status(),
            Self::FileChange(tool) => tool.status(),
            Self::Mcp(tool) => tool.status(),
            Self::Dynamic(tool) => tool.status(),
            Self::WebSearch(tool) => tool.status(),
            Self::ImageView(tool) => tool.status(),
            Self::Collaboration(tool) => tool.status(),
            Self::Sleep(tool) => tool.status(),
            Self::ImageGeneration(tool) => tool.status(),
        }
    }
}

impl RenderOnce for ToolCall {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        match self {
            Self::Command(tool) => tool.into_any_element(),
            Self::FileChange(tool) => tool.into_any_element(),
            Self::Mcp(tool) => SimpleToolElement::new(tool).into_any_element(),
            Self::Dynamic(tool) => SimpleToolElement::new(tool).into_any_element(),
            Self::WebSearch(tool) => SimpleToolElement::new(tool).into_any_element(),
            Self::ImageView(tool) => SimpleToolElement::new(tool).into_any_element(),
            Self::Collaboration(tool) => SimpleToolElement::new(tool).into_any_element(),
            Self::Sleep(tool) => SimpleToolElement::new(tool).into_any_element(),
            Self::ImageGeneration(tool) => tool.into_any_element(),
        }
    }
}

pub(super) fn render_group(
    key: &str,
    tools: &[ToolCall],
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
    let title_style = gpui::StyleRefinement::default().px_0().py_1();
    let content_style = gpui::StyleRefinement::default().px_0().pb_0();
    let accordion = Accordion::new(format!("tool-group-{key}"))
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
                .child(render_list(tools).p_5())
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

fn render_summary(tools: &[ToolCall], theme: &Theme) -> gpui::Div {
    let running = tools
        .iter()
        .filter(|tool| matches!(tool.status(), ToolStatus::Running))
        .count();
    let failed = tools
        .iter()
        .filter(|tool| matches!(tool.status(), ToolStatus::Failed))
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

fn render_list(tools: &[ToolCall]) -> gpui::Div {
    div()
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .items_stretch()
        .gap_1p5()
        .children(tools.iter().cloned())
}

pub(in crate::gui::chat_history) fn tool_calls(
    tools: &[&ThreadItem],
    progress: &HashMap<String, Vec<SharedString>>,
    is_streaming: impl Fn(&str) -> bool,
) -> Arc<[ToolCall]> {
    tools
        .iter()
        .filter_map(|tool| {
            ToolCall::new(
                tool,
                progress.get(tool.id()).map(Vec::as_slice),
                is_streaming(tool.id()),
            )
        })
        .collect()
}

pub(in crate::gui::chat_history) fn is_tool_item(item: &ThreadItem) -> bool {
    matches!(
        item,
        ThreadItem::CommandExecution { .. }
            | ThreadItem::FileChange { .. }
            | ThreadItem::McpToolCall { .. }
            | ThreadItem::DynamicToolCall { .. }
            | ThreadItem::WebSearch(_)
            | ThreadItem::ImageView { .. }
            | ThreadItem::CollabAgentToolCall { .. }
            | ThreadItem::Sleep(_)
            | ThreadItem::ImageGeneration(_)
    )
}

pub(in crate::gui::chat_history) fn tools_done(
    tools: &[&ThreadItem],
    is_streaming: impl Fn(&str) -> bool,
) -> bool {
    !tools.is_empty()
        && tools
            .iter()
            .all(|tool| tool_status(tool, is_streaming(tool.id())).done())
}

fn tool_status(item: &ThreadItem, streaming: bool) -> ToolStatus {
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
        ThreadItem::WebSearch(_) | ThreadItem::ImageView { .. } if streaming => ToolStatus::Running,
        ThreadItem::WebSearch(_) | ThreadItem::ImageView { .. } => ToolStatus::Succeeded,
        ThreadItem::CollabAgentToolCall { status, .. } => match status {
            CollabAgentToolCallStatus::InProgress => ToolStatus::Running,
            CollabAgentToolCallStatus::Completed => ToolStatus::Succeeded,
            CollabAgentToolCallStatus::Failed => ToolStatus::Failed,
        },
        ThreadItem::Sleep(_) if streaming => ToolStatus::Running,
        ThreadItem::Sleep(_) => ToolStatus::Succeeded,
        ThreadItem::ImageGeneration(item)
            if streaming || item.status.eq_ignore_ascii_case("in_progress") =>
        {
            ToolStatus::Running
        }
        ThreadItem::ImageGeneration(item)
            if item.status.eq_ignore_ascii_case("failed")
                || item.status.eq_ignore_ascii_case("error") =>
        {
            ToolStatus::Failed
        }
        ThreadItem::ImageGeneration(_) => ToolStatus::Succeeded,
        _ => ToolStatus::Failed,
    }
}

fn pluralize(count: usize, singular: &'static str) -> &'static str {
    if count == 1 { singular } else { "tool calls" }
}
