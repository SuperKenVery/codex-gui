use gpui::{IntoElement, ParentElement, SharedString, Styled, div, prelude::*, px};
use gpui_component::{Icon, IconName, Sizable as _, StyledExt as _, h_flex, theme::Theme};

pub(super) fn render_hook_prompt(body: SharedString, theme: &Theme) -> impl IntoElement {
    div().w_full().min_w_0().py_2().child(
        div()
            .w_full()
            .min_w_0()
            .rounded_lg()
            .border_1()
            .border_color(theme.info.opacity(0.35))
            .bg(theme.info.opacity(0.08))
            .px_4()
            .py_3()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                h_flex()
                    .gap_1p5()
                    .text_sm()
                    .font_semibold()
                    .text_color(theme.info)
                    .child(Icon::new(IconName::Info).xsmall())
                    .child("Hook context"),
            )
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
