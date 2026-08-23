use gpui::{App, IntoElement, ParentElement, Styled, div};
use gpui_component::ActiveTheme as _;

pub(super) fn render(label: &'static str, cx: &mut App) -> gpui::AnyElement {
    div()
        .flex()
        .items_center()
        .size_full()
        .px_2()
        .text_xs()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(cx.theme().muted_foreground)
        .child(label)
        .into_any_element()
}
