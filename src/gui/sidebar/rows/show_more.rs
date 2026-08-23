use gpui::{App, Context, IntoElement, ParentElement, Styled, WeakEntity, div, px};
use gpui_component::{
    ActiveTheme as _, Sizable as _,
    button::{Button, ButtonVariants as _},
};

use super::super::{PaginateKind, Sidebar};

pub(super) fn render(
    kind: PaginateKind,
    sidebar: &WeakEntity<Sidebar>,
    cx: &mut App,
) -> gpui::AnyElement {
    let more = pager_button(
        format!("show-more-{kind:?}"),
        "Show more",
        sidebar,
        kind.clone(),
        |view, kind, cx| view.show_more(kind, cx),
        cx,
    );
    let all = pager_button(
        format!("show-all-{kind:?}"),
        "Show all",
        sidebar,
        kind,
        |view, kind, cx| view.show_all(kind, cx),
        cx,
    );
    div()
        .flex()
        .gap_2()
        .w_full()
        .h_full()
        .child(more)
        .child(all)
        .into_any_element()
}

fn pager_button(
    id: impl Into<gpui::ElementId>,
    label: &'static str,
    sidebar: &WeakEntity<Sidebar>,
    kind: PaginateKind,
    action: impl Fn(&mut Sidebar, PaginateKind, &mut Context<Sidebar>) + 'static,
    cx: &App,
) -> Button {
    let sidebar = sidebar.clone();
    let kind = kind.clone();
    Button::new(id)
        .ghost()
        .h_full()
        .flex_1()
        .with_size(px(0.))
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .size_full()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .on_click(move |_, _, cx| {
            let kind = kind.clone();
            let _ = sidebar.update(cx, |view, cx| action(view, kind, cx));
        })
}
