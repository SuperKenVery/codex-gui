use std::time::Duration;

use crate::gui::{Message, MessageState, StreamState, ToolCall, ToolStatus};
use gpui::{
    App, ClickEvent, IntoElement, ParentElement, SharedString, Styled, Window, div, prelude::*, px,
};
use gpui_component::{
    Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    clipboard::Clipboard,
    h_flex,
    text::markdown,
    theme::Theme,
    v_flex,
};

pub(super) fn chat_tree_item(
    id: impl Into<gpui::ElementId>,
    title: SharedString,
    subtitle: SharedString,
    selected: bool,
    theme: &Theme,
) -> Button {
    Button::new(id)
        .ghost()
        .selected(selected)
        .with_size(px(0.))
        .w_full()
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
                )
                .child(
                    div()
                        .w_full()
                        .text_xs()
                        .line_height(px(16.))
                        .text_color(theme.muted_foreground)
                        .overflow_x_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(subtitle),
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

pub(super) fn render_message(message: &Message, theme: &Theme) -> impl IntoElement {
    render_message_view(message, false, false, false, theme, None)
}

pub(super) fn render_message_state(
    message: &MessageState,
    collapse_tools: bool,
    hide_tools: bool,
    active_tool_tail: bool,
    theme: &Theme,
    on_toggle_tools: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    render_message_view(
        &message.message,
        collapse_tools,
        hide_tools,
        active_tool_tail,
        theme,
        Some((message.tools_expanded, Box::new(on_toggle_tools))),
    )
}

pub(super) fn render_worked_summary(duration: Duration, theme: &Theme) -> gpui::Div {
    div()
        .w_full()
        .min_w_0()
        .py_1()
        .text_sm()
        .text_color(theme.muted_foreground)
        .child(format!("Worked for {}", format_duration(duration)))
}

fn render_message_view(
    message: &Message,
    collapse_tools: bool,
    hide_tools: bool,
    active_tool_tail: bool,
    theme: &Theme,
    tool_toggle: Option<(
        bool,
        Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>,
    )>,
) -> gpui::Div {
    match message {
        Message::User(body) => message_block("You", body, theme, MessageBodyFormat::Plain),
        Message::Commentary(body) => {
            message_block("Commentary", body, theme, MessageBodyFormat::Markdown)
        }
        Message::Assistant {
            body, state, tools, ..
        } => {
            let mut block = message_block(
                match state {
                    StreamState::Complete => "Codex",
                    StreamState::Streaming => "Codex is working",
                },
                body,
                theme,
                MessageBodyFormat::Markdown,
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
                            .id(format!("tool-summary-{}", tools[0].id))
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
    }
}

enum MessageBodyFormat {
    Plain,
    Markdown,
}

fn message_block(
    author: &'static str,
    body: &str,
    theme: &Theme,
    body_format: MessageBodyFormat,
) -> gpui::Div {
    let body = match body_format {
        MessageBodyFormat::Plain => div()
            .text_sm()
            .line_height(px(22.))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .whitespace_normal()
            .child(body.to_string())
            .into_any_element(),
        MessageBodyFormat::Markdown => markdown(body.to_string())
            .selectable(true)
            .code_block_actions(|code_block, _, _| {
                h_flex()
                    .gap_1()
                    .child(Clipboard::new("copy-code").value(code_block.code().clone()))
            })
            .into_any_element(),
    };

    div()
        .w_full()
        .min_w_0()
        .overflow_x_hidden()
        .py_2()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .min_w_0()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(author),
        )
        .child(div().w_full().min_w_0().overflow_x_hidden().child(body))
}

fn render_tool_summary(tools: &[ToolCall], theme: &Theme, expanded: bool) -> gpui::Div {
    let running = tools
        .iter()
        .filter(|tool| matches!(tool.status, ToolStatus::Running))
        .count();
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

    let indicator = if expanded { "^" } else { "v" };

    div()
        .w_full()
        .min_w_0()
        .cursor_pointer()
        .text_sm()
        .text_color(theme.muted_foreground)
        .child(format!("{label}  {indicator}"))
}

fn render_tool_list(tools: &[ToolCall], theme: &Theme) -> gpui::Div {
    tools
        .iter()
        .fold(div().flex().flex_col().gap_2(), |list, tool| {
            list.child(render_tool_call(tool, theme))
        })
}

fn render_tool_call(tool: &ToolCall, theme: &Theme) -> gpui::Div {
    let (label, color) = match tool.status {
        ToolStatus::Running => ("running", theme.warning_foreground),
        ToolStatus::Done => ("done", theme.success_foreground),
    };

    div()
        .w_full()
        .min_w_0()
        .overflow_x_hidden()
        .py_1()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .min_w_0()
                        .overflow_x_hidden()
                        .text_sm()
                        .whitespace_normal()
                        .child(tool.name.clone()),
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
        .child(div().text_color(color).text_xs().child(label))
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
