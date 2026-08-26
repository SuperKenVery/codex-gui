use gpui::{IntoElement, ParentElement, SharedString, Styled, div, prelude::*, px};
use gpui_component::{StyledExt as _, theme::Theme};

pub(super) fn render(
    title: SharedString,
    body: SharedString,
    running: bool,
    theme: &Theme,
) -> impl IntoElement {
    div().w_full().min_w_0().py_2().child(
        div()
            .w_full()
            .min_w_0()
            .rounded_lg()
            .bg(theme.muted)
            .px_4()
            .py_3()
            .flex()
            .flex_col()
            .gap_1()
            .child(div().text_sm().font_semibold().child(if running {
                format!("{title}…")
            } else {
                title.to_string()
            }))
            .when(!body.is_empty(), |this| {
                this.child(
                    div()
                        .text_sm()
                        .whitespace_normal()
                        .line_height(px(21.))
                        .child(body),
                )
            }),
    )
}
