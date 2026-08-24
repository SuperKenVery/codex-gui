use gpui::{IntoElement, ParentElement, SharedString, Styled, div, px};
use gpui_component::theme::Theme;

pub(super) fn render(body: SharedString, theme: &Theme) -> impl IntoElement {
    div().w_full().min_w_0().overflow_x_hidden().py_2().child(
        div()
            .w_full()
            .min_w_0()
            .overflow_x_hidden()
            .rounded_lg()
            .bg(theme.danger_foreground)
            .px_4()
            .py_3()
            .text_base()
            .line_height(px(25.))
            // .text_color(theme.danger_foreground)
            .whitespace_normal()
            .child(body),
    )
}
