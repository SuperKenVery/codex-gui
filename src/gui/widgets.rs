use crate::models::{Message, StreamState, ToolCall, ToolStatus};
use gpui::{IntoElement, ParentElement, SharedString, Styled, div, prelude::*, px, rgb};
use gpui_component::{
    Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    v_flex,
};

pub(super) fn section_label(text: &'static str) -> impl IntoElement {
    div()
        .text_xs()
        .text_color(rgb(0x717b8f))
        .child(text.to_ascii_uppercase())
}

pub(super) fn nav_item(
    id: impl Into<gpui::ElementId>,
    title: SharedString,
    subtitle: SharedString,
    selected: bool,
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
                        .text_color(rgb(0x9aa3b5))
                        .whitespace_normal()
                        .child(subtitle),
                ),
        )
}

pub(super) fn command_button(id: &'static str, label: &'static str) -> Button {
    Button::new(id).small().label(label)
}

pub(super) fn status_pill(label: String) -> impl IntoElement {
    div()
        .px_2()
        .py_1()
        .rounded_sm()
        .bg(rgb(0x1d2430))
        .text_xs()
        .text_color(rgb(0x9ca8ba))
        .child(label)
}

pub(super) fn render_message(message: &Message) -> impl IntoElement {
    match message {
        Message::User(body) => message_card("YOU", body, rgb(0x1f2937).into(), None),
        Message::Commentary(body) => message_card("COMMENTARY", body, rgb(0x1b2430).into(), None),
        Message::Assistant {
            body, state, tools, ..
        } => {
            let tool_list = tools
                .iter()
                .fold(div().flex().flex_col().gap_2(), |list, tool| {
                    list.child(render_tool_call(tool))
                });
            message_card(
                match state {
                    StreamState::Complete => "CODEX",
                    StreamState::Streaming => "CODEX IS WORKING",
                },
                body,
                rgb(0x161b23).into(),
                Some(tool_list),
            )
        }
    }
}

fn message_card(
    author: &'static str,
    body: &str,
    background: gpui::Hsla,
    child: Option<gpui::Div>,
) -> impl IntoElement {
    div()
        .rounded_md()
        .border_1()
        .border_color(rgb(0x2a303b))
        .bg(background)
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .child(div().text_xs().text_color(rgb(0x7c879a)).child(author))
        .child(div().text_sm().line_height(px(22.)).child(body.to_string()))
        .when_some(child, |this, child| this.child(child))
}

fn render_tool_call(tool: &ToolCall) -> impl IntoElement {
    let (label, color) = match tool.status {
        ToolStatus::Running => ("running", gpui::Hsla::from(rgb(0xf59e0b))),
        ToolStatus::Done => ("done", gpui::Hsla::from(rgb(0x22c55e))),
    };

    div()
        .rounded_sm()
        .border_1()
        .border_color(rgb(0x303746))
        .bg(rgb(0x111722))
        .p_3()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_sm().child(tool.name.clone()))
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(0x919bad))
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
