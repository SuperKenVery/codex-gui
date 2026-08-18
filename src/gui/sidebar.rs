use crate::app::CodexGui;
use crate::gui::{ChatState, GuiState, ProjectState, widgets::chat_tree_item};
use gpui::{
    App, Context, Entity, IntoElement, ListAlignment, ListState, MouseButton, ParentElement,
    Render, Styled, Subscription, WeakEntity, Window, WindowControlArea, div, list, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    scroll::ScrollableElement as _,
};
use std::{collections::HashSet, sync::Arc};

const SIDEBAR_ROW_HEIGHT: gpui::Pixels = px(34.);
const SIDEBAR_LIST_OVERDRAW: gpui::Pixels = px(170.);

pub struct Sidebar {
    parent: WeakEntity<CodexGui>,
    state: Entity<GuiState>,
    should_move_window: bool,
    collapsed_projects: HashSet<String>,
    expanded_thread_projects: HashSet<String>,
    list_state: ListState,
    list_active_project: Option<usize>,
    _subscriptions: Vec<Subscription>,
}

#[derive(Clone)]
enum SidebarRow<Project, Chat> {
    Project {
        project_index: usize,
        project: Project,
        selected: bool,
        expanded: bool,
    },
    Chat {
        project_index: usize,
        chat_index: usize,
        chat: Chat,
        selected: bool,
    },
    ShowMore {
        project_index: usize,
        path: String,
    },
}

impl Sidebar {
    pub fn new(
        parent: WeakEntity<CodexGui>,
        state: Entity<GuiState>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = vec![cx.observe(&state, |_, _, cx| cx.notify())];
        Self {
            parent,
            state,
            should_move_window: false,
            collapsed_projects: HashSet::new(),
            expanded_thread_projects: HashSet::new(),
            list_state: ListState::new(0, ListAlignment::Top, SIDEBAR_LIST_OVERDRAW)
                .with_uniform_item_height(SIDEBAR_ROW_HEIGHT),
            list_active_project: None,
            _subscriptions: subscriptions,
        }
    }

    fn toggle_project(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some((path, is_active)) = self.state.read(cx).projects.get(index).map(|project| {
            (
                project.read(cx).path.to_string(),
                index == self.state.read(cx).active_project,
            )
        }) else {
            return;
        };

        if is_active {
            if !self.collapsed_projects.insert(path.clone()) {
                self.collapsed_projects.remove(&path);
            }
            cx.notify();
            return;
        }

        self.collapsed_projects.remove(&path);
        let parent = self.parent.clone();
        cx.defer(move |cx| {
            let _ = parent.update(cx, |parent, cx| parent.select_project(index, cx));
        });
    }

    fn show_all_threads(&mut self, path: String, cx: &mut Context<Self>) {
        self.expanded_thread_projects.insert(path);
        cx.notify();
    }

    fn select_chat(&mut self, index: usize, cx: &mut Context<Self>) {
        let parent = self.parent.clone();
        cx.defer(move |cx| {
            let _ = parent.update(cx, |parent, cx| parent.select_chat(index, cx));
        });
    }

    fn open_new_chat(&mut self, cx: &mut Context<Self>) {
        let parent = self.parent.clone();
        cx.defer(move |cx| {
            let _ = parent.update(cx, |parent, cx| parent.open_new_chat(cx));
        });
    }
}

fn build_sidebar_rows<Project: Clone, Chat: Clone>(
    projects: &[Project],
    active_project: usize,
    active_chat: usize,
    active_project_path: Option<&str>,
    active_project_chats: &[Chat],
    collapsed_projects: &HashSet<String>,
    expanded_thread_projects: &HashSet<String>,
) -> Vec<SidebarRow<Project, Chat>> {
    let mut rows = Vec::with_capacity(projects.len() + 6);

    for (project_index, project) in projects.iter().enumerate() {
        let selected = project_index == active_project;
        let expanded =
            selected && active_project_path.is_some_and(|path| !collapsed_projects.contains(path));
        rows.push(SidebarRow::Project {
            project_index,
            project: project.clone(),
            selected,
            expanded,
        });

        if !expanded {
            continue;
        }

        let path = active_project_path.expect("an expanded project always has a path");
        let show_all = expanded_thread_projects.contains(path);
        let visible_thread_count = if show_all {
            active_project_chats.len()
        } else {
            active_project_chats.len().min(5)
        };
        rows.extend(
            active_project_chats
                .iter()
                .take(visible_thread_count)
                .enumerate()
                .map(|(chat_index, chat)| SidebarRow::Chat {
                    project_index,
                    chat_index,
                    chat: chat.clone(),
                    selected: chat_index == active_chat,
                }),
        );
        if active_project_chats.len() > visible_thread_count {
            rows.push(SidebarRow::ShowMore {
                project_index,
                path: path.to_owned(),
            });
        }
    }

    rows
}

fn render_sidebar_row(
    row: SidebarRow<Entity<ProjectState>, Entity<ChatState>>,
    sidebar: &WeakEntity<Sidebar>,
    cx: &mut App,
) -> gpui::AnyElement {
    let content = match row {
        SidebarRow::Project {
            project_index,
            project,
            selected,
            expanded,
        } => {
            let (name, path) = {
                let project = project.read(cx);
                (project.name.clone(), project.path.clone())
            };
            let sidebar = sidebar.clone();
            Button::new(format!("project-{project_index}"))
                .ghost()
                .tooltip(path)
                .with_size(px(0.))
                .w_full()
                .h_full()
                .rounded_lg()
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
        SidebarRow::Chat {
            project_index,
            chat_index,
            chat,
            selected,
        } => {
            let (title, subtitle) = {
                let chat = chat.read(cx);
                (chat.title.clone(), chat.subtitle.clone())
            };
            let sidebar = sidebar.clone();
            chat_tree_item(
                format!("chat-{project_index}-{chat_index}"),
                title,
                subtitle,
                selected,
                cx.theme(),
            )
            .h_full()
            .on_click(move |_, _, cx| {
                let _ = sidebar.update(cx, |view, cx| view.select_chat(chat_index, cx));
            })
            .into_any_element()
        }
        SidebarRow::ShowMore {
            project_index,
            path,
        } => {
            let sidebar = sidebar.clone();
            Button::new(format!("show-more-threads-{project_index}"))
                .ghost()
                .w_full()
                .h_full()
                .with_size(px(0.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .size_full()
                        .pl_7()
                        .pr_2()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("显示更多"),
                )
                .on_click(move |_, _, cx| {
                    let path = path.clone();
                    let _ = sidebar.update(cx, |view, cx| view.show_all_threads(path, cx));
                })
                .into_any_element()
        }
    };

    div()
        .h(SIDEBAR_ROW_HEIGHT)
        .w_full()
        .pr_1()
        .child(content)
        .into_any_element()
}

impl Render for Sidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (projects, active_project, active_chat) = {
            let state = self.state.read(cx);
            (
                state.projects.clone(),
                state.active_project,
                state.active_chat,
            )
        };
        let (active_project_path, active_project_chats) = projects
            .get(active_project)
            .map(|project| {
                let project = project.read(cx);
                (Some(project.path.to_string()), project.chats.clone())
            })
            .unwrap_or_default();
        let rows = Arc::new(build_sidebar_rows(
            &projects,
            active_project,
            active_chat,
            active_project_path.as_deref(),
            &active_project_chats,
            &self.collapsed_projects,
            &self.expanded_thread_projects,
        ));
        let item_count_changed = self.list_state.item_count() != rows.len();
        if item_count_changed {
            self.list_state
                .reset_with_uniform_height(rows.len(), SIDEBAR_ROW_HEIGHT);
        }
        if !rows.is_empty()
            && (item_count_changed || self.list_active_project != Some(active_project))
        {
            self.list_state
                .scroll_to_reveal_item(active_project.min(rows.len() - 1));
        }
        self.list_active_project = Some(active_project);
        let sidebar = cx.entity().downgrade();

        div()
            .w(px(286.))
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(cx.theme().border.opacity(0.35))
            .bg(cx.theme().sidebar.opacity(0.28))
            .text_color(cx.theme().sidebar_foreground)
            .px_3()
            .pb_4()
            .gap_4()
            .child(
                div()
                    .window_control_area(WindowControlArea::Drag)
                    .on_mouse_down_out(cx.listener(|view, _, _, _| {
                        view.should_move_window = false;
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, _, _, _| {
                            view.should_move_window = true;
                        }),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|view, _, _, _| {
                            view.should_move_window = false;
                        }),
                    )
                    .on_mouse_move(cx.listener(|view, _, window, _| {
                        if view.should_move_window {
                            view.should_move_window = false;
                            window.start_window_move();
                        }
                    }))
                    .h(px(20.))
                    .w_full()
                    .flex()
                    .items_center(),
            )
            .child(
                Button::new("new-chat")
                    .ghost()
                    .w_full()
                    .with_size(px(0.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .w_full()
                            .py_2()
                            .child(Icon::new(IconName::Plus).small())
                            .child(div().text_sm().child("New chat")),
                    )
                    .on_click(cx.listener(|view, _, _, cx| view.open_new_chat(cx))),
            )
            .child(
                div()
                    .id("sidebar-scroll")
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .child(
                        list(self.list_state.clone(), move |index, _, cx| {
                            rows.get(index)
                                .cloned()
                                .map(|row| render_sidebar_row(row, &sidebar, cx))
                                .unwrap_or_else(|| div().into_any_element())
                        })
                        .size_full(),
                    )
                    .vertical_scrollbar(&self.list_state),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flattens_only_the_active_projects_visible_threads() {
        let projects = vec!["one", "two", "three"];
        let chats = (0..8).collect::<Vec<_>>();
        let rows = build_sidebar_rows(
            &projects,
            1,
            2,
            Some("two"),
            &chats,
            &HashSet::new(),
            &HashSet::new(),
        );

        assert_eq!(rows.len(), 9); // 3 projects + 5 chats + show-more.
        assert!(matches!(
            rows[1],
            SidebarRow::Project {
                project_index: 1,
                expanded: true,
                ..
            }
        ));
        assert!(matches!(
            rows[4],
            SidebarRow::Chat {
                chat_index: 2,
                selected: true,
                ..
            }
        ));
        assert!(matches!(rows[7], SidebarRow::ShowMore { .. }));
    }

    #[test]
    fn show_all_and_collapse_change_only_the_flattened_rows() {
        let projects = vec!["one", "two"];
        let chats = (0..8).collect::<Vec<_>>();
        let expanded = HashSet::from(["one".to_string()]);
        let rows = build_sidebar_rows(
            &projects,
            0,
            0,
            Some("one"),
            &chats,
            &HashSet::new(),
            &expanded,
        );
        assert_eq!(rows.len(), 10); // 2 projects + all 8 chats.
        assert!(
            !rows
                .iter()
                .any(|row| matches!(row, SidebarRow::ShowMore { .. }))
        );

        let collapsed = HashSet::from(["one".to_string()]);
        let rows = build_sidebar_rows(&projects, 0, 0, Some("one"), &chats, &collapsed, &expanded);
        assert_eq!(rows.len(), 2);
        assert!(
            rows.iter()
                .all(|row| matches!(row, SidebarRow::Project { .. }))
        );
    }
}
