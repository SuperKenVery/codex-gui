use gpui::{IntoElement, ParentElement, Styled, div};
use gpui_component::theme::Theme;

pub(super) fn render_notice(body: &str, theme: &Theme) -> impl IntoElement {
    div()
        .w_full()
        .h_full()
        .overflow_x_hidden()
        .flex()
        .justify_center()
        .items_center()
        .text_lg()
        .text_color(theme.foreground)
        .child(body.to_string())
}
