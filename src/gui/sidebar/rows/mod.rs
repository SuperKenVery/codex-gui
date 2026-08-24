mod chat;
mod model;
mod project;
mod section_header;
mod show_more;

use gpui::{App, Entity, IntoElement, ParentElement, Styled, WeakEntity, div, px};
use gpui_component::{
    Sizable as _,
    button::{Button, ButtonVariants as _},
};

use super::{SIDEBAR_ROW_GAP, SIDEBAR_ROW_HEIGHT, Sidebar};
use crate::gui::{ChatState, ProjectState};
pub(super) use model::{PaginateKind, SidebarRow, SidebarRowDisplayStatus};

/// The base button style shared by all interactive sidebar rows
/// (project folders and chat threads).
pub(super) fn row_button(id: impl Into<gpui::ElementId>) -> Button {
    Button::new(id)
        .ghost()
        .with_size(px(0.))
        .w_full()
        .h_full()
        .rounded_lg()
}

pub(super) fn render_sidebar_row(
    row: SidebarRow<Entity<ProjectState>, Entity<ChatState>>,
    sidebar: &WeakEntity<Sidebar>,
    cx: &mut App,
) -> gpui::AnyElement {
    let content = match row {
        SidebarRow::SectionHeader { label } => section_header::render(label, cx),
        SidebarRow::Project {
            project_index,
            project,
            selected,
            expanded,
        } => project::render(project_index, project, selected, expanded, sidebar, cx),
        SidebarRow::Chat {
            project_index,
            chat_index,
            chat,
            selected,
        } => chat::render(project_index, chat_index, chat, selected, sidebar, cx),
        SidebarRow::ProjectlessChat {
            chat_index,
            chat,
            selected,
        } => chat::render_projectless(chat_index, chat, selected, sidebar, cx),
        SidebarRow::ShowMore { kind } => show_more::render(kind, sidebar, cx),
    };

    div()
        .h(SIDEBAR_ROW_HEIGHT)
        .w_full()
        .pr_1()
        .py(SIDEBAR_ROW_GAP)
        .child(content)
        .into_any_element()
}
