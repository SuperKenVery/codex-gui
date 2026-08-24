use gpui::{App, ClickEvent, ClipboardItem, ParentElement, SharedString, Styled, Window, div, px};
use gpui_component::{
    Sizable as _,
    button::{Button, ButtonVariants as _},
    theme::Theme,
};

pub(super) fn render_assistant_header(author: &'static str, theme: &Theme) -> gpui::Div {
    div()
        .w_full()
        .min_w_0()
        .pt_2()
        .text_xs()
        .text_color(theme.muted_foreground)
        .child(author)
}

pub(super) fn render_user(
    key: &str,
    body: SharedString,
    theme: &Theme,
    on_edit: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_fork: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::Div {
    let copy_body = body.clone();

    div()
        .w_full()
        .min_w_0()
        .overflow_x_hidden()
        .py_2()
        .flex()
        .justify_end()
        .child(
            div()
                .w_full()
                .max_w(px(620.))
                .min_w_0()
                .flex()
                .flex_col()
                .items_end()
                .child(
                    div()
                        .min_w_0()
                        .overflow_x_hidden()
                        .rounded_3xl()
                        .bg(theme.secondary)
                        .px_3()
                        .py_2()
                        .text_base()
                        .line_height(px(25.))
                        .text_color(theme.secondary_foreground)
                        .whitespace_normal()
                        .child(body),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .pt_1()
                        .pr_1()
                        .child(
                            Button::new(format!("copy-user-message-{key}"))
                                .xsmall()
                                .ghost()
                                .label("Copy")
                                .tooltip("Copy message")
                                .on_click(move |_, _, cx| {
                                    cx.stop_propagation();
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        copy_body.to_string(),
                                    ));
                                }),
                        )
                        .child(
                            Button::new(format!("edit-user-message-{key}"))
                                .xsmall()
                                .ghost()
                                .label("Edit")
                                .tooltip("Edit message in a fork")
                                .on_click(on_edit),
                        )
                        .child(
                            Button::new(format!("fork-user-message-{key}"))
                                .xsmall()
                                .ghost()
                                .label("Fork")
                                .tooltip("Fork chat from this message")
                                .on_click(on_fork),
                        ),
                ),
        )
}
