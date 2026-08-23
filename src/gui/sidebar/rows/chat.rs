use gpui::{
    App, Entity, IntoElement, ParentElement, SharedString, Styled, WeakEntity, div, prelude::*, px,
};
use gpui_component::{ActiveTheme as _, button::Button, theme::Theme, v_flex};

use super::super::Sidebar;
use super::row_button;
use crate::gui::ChatState;

pub(super) fn render(
    project_index: usize,
    chat_index: usize,
    chat: Entity<ChatState>,
    selected: bool,
    sidebar: &WeakEntity<Sidebar>,
    cx: &mut App,
) -> gpui::AnyElement {
    let title = chat.read(cx).title.clone();
    let sidebar = sidebar.clone();
    chat_tree_item(
        format!("chat-{project_index}-{chat_index}"),
        title,
        selected,
        cx.theme(),
    )
    .on_click(move |_, _, cx| {
        let _ = sidebar.update(cx, |view, cx| view.select_chat(chat_index, cx));
    })
    .into_any_element()
}

pub(super) fn render_projectless(
    chat_index: usize,
    chat: Entity<ChatState>,
    selected: bool,
    sidebar: &WeakEntity<Sidebar>,
    cx: &mut App,
) -> gpui::AnyElement {
    let title = chat.read(cx).title.clone();
    let sidebar = sidebar.clone();
    chat_tree_item(
        format!("projectless-chat-{chat_index}"),
        title,
        selected,
        cx.theme(),
    )
    .on_click(move |_, _, cx| {
        let _ = sidebar.update(cx, |view, cx| view.select_projectless_chat(chat_index, cx));
    })
    .into_any_element()
}

/// The shared chat row button, indented to sit under its project folder.
fn chat_tree_item(
    id: impl Into<gpui::ElementId>,
    title: SharedString,
    selected: bool,
    theme: &Theme,
) -> Button {
    row_button(id)
        .when(selected, |this| this.bg(theme.sidebar_accent.opacity(0.26)))
        .child(
            v_flex()
                .w_full()
                .min_w_0()
                .gap_0p5()
                .items_start()
                .rounded_lg()
                .py_1p5()
                .pl_7()
                .pr_2()
                .child(
                    div()
                        .w_full()
                        .text_sm()
                        .line_height(px(18.))
                        .overflow_x_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(title),
                ),
        )
}
