use gpui::{App, IntoElement, ParentElement, SharedString, Styled, div};
use gpui_component::alert::Alert;

pub(super) fn render(
    key: &str,
    body: SharedString,
    on_dismiss: impl Fn(&mut App) + 'static,
) -> impl IntoElement {
    div().w_full().min_w_0().overflow_x_hidden().py_2().child(
        Alert::warning(key.to_string(), body).on_close(move |_, _, cx| {
            cx.stop_propagation();
            on_dismiss(cx);
        }),
    )
}
