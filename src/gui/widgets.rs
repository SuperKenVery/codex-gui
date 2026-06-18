use crate::gui::{Message, StreamState, ToolCall, ToolStatus};
use gpui::{IntoElement, ParentElement, SharedString, Styled, div, prelude::*, px};
use gpui_component::{
    Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    theme::Theme,
    v_flex,
};

pub(super) fn section_label(text: &'static str, theme: &Theme) -> impl IntoElement {
    div()
        .text_xs()
        .text_color(theme.muted_foreground)
        .child(text.to_ascii_uppercase())
}

pub(super) fn nav_item(
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
                .gap_1()
                .items_start()
                .py_2()
                .child(
                    div()
                        .w_full()
                        .text_sm()
                        .line_height(px(18.))
                        .whitespace_normal()
                        .child(title),
                )
                .child(
                    div()
                        .w_full()
                        .text_xs()
                        .line_height(px(16.))
                        .text_color(theme.muted_foreground)
                        .whitespace_normal()
                        .child(subtitle),
                ),
        )
}

pub(super) fn command_button(id: &'static str, label: &'static str) -> Button {
    Button::new(id).small().label(label)
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
    match message {
        Message::User(body) => message_card("YOU", body, theme.accent, None, theme),
        Message::Commentary(body) => message_card("COMMENTARY", body, theme.secondary, None, theme),
        Message::Assistant {
            body, state, tools, ..
        } => {
            let tool_list = tools
                .iter()
                .fold(div().flex().flex_col().gap_2(), |list, tool| {
                    list.child(render_tool_call(tool, theme))
                });
            message_card(
                match state {
                    StreamState::Complete => "CODEX",
                    StreamState::Streaming => "CODEX IS WORKING",
                },
                body,
                theme.background,
                Some(tool_list),
                theme,
            )
        }
    }
}

fn message_card(
    author: &'static str,
    body: &str,
    background: gpui::Hsla,
    child: Option<gpui::Div>,
    theme: &Theme,
) -> impl IntoElement {
    div()
        .w_full()
        .min_w_0()
        .overflow_x_hidden()
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .bg(background)
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .min_w_0()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(author),
        )
        .child(
            div()
                .w_full()
                .min_w_0()
                .overflow_x_hidden()
                .text_sm()
                .line_height(px(22.))
                .whitespace_normal()
                .child(body.to_string()),
        )
        .when_some(child, |this, child| this.child(child))
}

fn render_tool_call(tool: &ToolCall, theme: &Theme) -> impl IntoElement {
    let (label, color) = match tool.status {
        ToolStatus::Running => ("running", theme.warning_foreground),
        ToolStatus::Done => ("done", theme.success_foreground),
    };

    div()
        .w_full()
        .min_w_0()
        .overflow_x_hidden()
        .rounded_sm()
        .border_1()
        .border_color(theme.border)
        .bg(theme.secondary)
        .p_3()
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
        .child(
            div()
                .px_2()
                .py_1()
                .rounded_sm()
                .bg(color.opacity(0.18))
                .text_color(color)
                .text_xs()
                .child(label),
        )
}
