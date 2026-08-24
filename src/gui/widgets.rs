use gpui::{IntoElement, ParentElement, Styled, div};
use gpui_component::theme::Theme;

pub(super) fn render_notice(body: &str, theme: &Theme) -> impl IntoElement {
    div()
        .w_full()
        .min_w_0()
        .overflow_x_hidden()
        .py_2()
        .text_sm()
        .text_color(theme.foreground)
        .child(body.to_string())
}
