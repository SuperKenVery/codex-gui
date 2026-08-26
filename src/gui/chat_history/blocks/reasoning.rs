use gpui::{IntoElement, ParentElement, SharedString, Styled, div, prelude::*, px};
use gpui_component::{StyledExt as _, theme::Theme};

pub(super) fn render(body: SharedString, running: bool, theme: &Theme) -> impl IntoElement {
    div().w_full().min_w_0().py_2().child(
        div()
            .w_full()
            .min_w_0()
            .border_l_2()
            .border_color(theme.border)
            .pl_3()
            .py_1()
            .flex()
            .flex_col()
            .gap_1()
            .text_color(theme.muted_foreground)
            .child(div().text_sm().font_semibold().child(if running {
                "Reasoning…"
            } else {
                "Reasoning"
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
