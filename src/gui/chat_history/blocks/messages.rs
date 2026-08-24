use gpui::{ParentElement, SharedString, Styled, div, px};
use gpui_component::theme::Theme;

pub(super) fn render_assistant_header(author: &'static str, theme: &Theme) -> gpui::Div {
    div()
        .w_full()
        .min_w_0()
        .pt_2()
        .text_xs()
        .text_color(theme.muted_foreground)
        .child(author)
}

pub(super) fn render_user(body: SharedString, theme: &Theme) -> gpui::Div {
    div()
        .w_full()
        .min_w_0()
        .overflow_x_hidden()
        .py_2()
        .flex()
        .justify_end()
        .child(
            div()
                .max_w(px(620.))
                .min_w_0()
                .overflow_x_hidden()
                .rounded_lg()
                .bg(theme.secondary)
                .px_3()
                .py_2()
                .text_base()
                .line_height(px(30.))
                .text_color(theme.secondary_foreground)
                .whitespace_normal()
                .child(body),
        )
}
