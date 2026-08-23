use gpui::{App, Entity, IntoElement, ParentElement, Styled, WeakEntity, div, prelude::*};
use gpui_component::{ActiveTheme as _, Icon, IconName, Sizable as _};

use super::super::Sidebar;
use super::row_button;
use crate::gui::ProjectState;

pub(super) fn render(
    project_index: usize,
    project: Entity<ProjectState>,
    selected: bool,
    expanded: bool,
    sidebar: &WeakEntity<Sidebar>,
    cx: &mut App,
) -> gpui::AnyElement {
    let (name, path) = {
        let project = project.read(cx);
        (project.name.clone(), project.path.clone())
    };
    let sidebar = sidebar.clone();
    row_button(format!("project-{project_index}"))
        .tooltip(path)
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .size_full()
                .min_w_0()
                .rounded_lg()
                .px_2()
                .when(selected, |this| {
                    this.bg(cx.theme().sidebar_accent.opacity(0.38))
                })
                .child(
                    Icon::new(if expanded {
                        IconName::FolderOpen
                    } else {
                        IconName::Folder
                    })
                    .small()
                    .text_color(cx.theme().muted_foreground),
                )
                .child(
                    div()
                        .min_w_0()
                        .text_sm()
                        .overflow_x_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(name),
                ),
        )
        .on_click(move |_, _, cx| {
            let _ = sidebar.update(cx, |view, cx| view.toggle_project(project_index, cx));
        })
        .into_any_element()
}
