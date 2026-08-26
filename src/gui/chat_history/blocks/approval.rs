use gpui::{IntoElement, ParentElement, SharedString, Styled, div, prelude::*, px};
use gpui_component::{
    Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    theme::Theme,
};

pub(super) fn render(
    key: &str,
    title: SharedString,
    body: SharedString,
    theme: &Theme,
    on_allow: impl Fn(&mut gpui::App) + Send + Sync + 'static,
    on_reject: impl Fn(&mut gpui::App) + Send + Sync + 'static,
) -> impl IntoElement {
    div().w_full().min_w_0().py_2().child(
        div()
            .w_full()
            .min_w_0()
            .rounded_lg()
            .border_1()
            .border_color(theme.warning_foreground)
            .bg(theme.muted)
            .px_4()
            .py_3()
            .flex()
            .flex_col()
            .gap_2()
            .child(div().font_semibold().text_base().child(title))
            .when(!body.is_empty(), |this| {
                this.child(
                    div()
                        .whitespace_normal()
                        .text_sm()
                        .line_height(px(21.))
                        .child(body),
                )
            })
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new(format!("approval-allow-{key}"))
                            .small()
                            .primary()
                            .label("Allow")
                            .on_click(move |_, _, cx| on_allow(cx)),
                    )
                    .child(
                        Button::new(format!("approval-reject-{key}"))
                            .small()
                            .danger()
                            .label("Reject")
                            .on_click(move |_, _, cx| on_reject(cx)),
                    ),
            ),
    )
}
