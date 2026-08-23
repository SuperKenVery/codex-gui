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
    spinner::Spinner,
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

const SIDEBAR_ROW_HEIGHT: gpui::Pixels = px(34.);
const SIDEBAR_LIST_OVERDRAW: gpui::Pixels = px(170.);
const PAGE_SIZE: usize = 10;

pub struct Sidebar {
    parent: WeakEntity<CodexGui>,
    state: Entity<GuiState>,
    should_move_window: bool,
    collapsed_projects: HashSet<String>,
    visible_project_count: usize,
    visible_projectless_count: usize,
    project_chat_visible_counts: HashMap<String, usize>,
    list_state: ListState,
    list_active_project: Option<usize>,
    _subscriptions: Vec<Subscription>,
}

#[derive(Clone, Debug)]
enum SidebarRow<Project, Chat> {
    /// "Projects" or "Chats" label
    SectionHeader { label: &'static str },
    /// Project folder that contains chats in it
    Project {
        project_index: usize,
        project: Project,
        selected: bool,
        expanded: bool,
    },
    /// A thread with a user-specified project dir
    Chat {
        project_index: usize,
        chat_index: usize,
        chat: Chat,
        selected: bool,
    },
    /// A project-less chat
    ProjectlessChat {
        chat_index: usize,
        chat: Chat,
        selected: bool,
    },
    /// Show more/show all button
    ShowMore { kind: PaginateKind },
}

#[derive(Clone, Debug)]
enum PaginateKind {
    Projects,
    ProjectlessChats,
    ProjectChats { path: String },
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
            visible_project_count: PAGE_SIZE,
            visible_projectless_count: PAGE_SIZE,
            project_chat_visible_counts: HashMap::new(),
            list_state: ListState::new(0, ListAlignment::Top, SIDEBAR_LIST_OVERDRAW)
                .with_uniform_item_height(SIDEBAR_ROW_HEIGHT),
            list_active_project: None,
            _subscriptions: subscriptions,
        }
    }

    fn toggle_project(&mut self, index: usize, cx: &mut Context<Self>) {
        let state = self.state.read(cx);
        let Some(project) = state.projects.get(index) else {
            return;
        };
        let path = project.read(cx).path.to_string();
        let is_active = index == state.active_project && state.active_projectless_chat.is_none();

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

    fn show_more(&mut self, kind: PaginateKind, cx: &mut Context<Self>) {
        match kind {
            PaginateKind::Projects => {
                self.visible_project_count = self.visible_project_count.saturating_add(PAGE_SIZE);
            }
            PaginateKind::ProjectlessChats => {
                self.visible_projectless_count =
                    self.visible_projectless_count.saturating_add(PAGE_SIZE);
            }
            PaginateKind::ProjectChats { path } => {
                let count = self
                    .project_chat_visible_counts
                    .entry(path)
                    .or_insert(PAGE_SIZE);
                *count = count.saturating_add(PAGE_SIZE);
            }
        }
        cx.notify();
    }

    fn show_all(&mut self, kind: PaginateKind, cx: &mut Context<Self>) {
        match kind {
            PaginateKind::Projects => self.visible_project_count = usize::MAX,
            PaginateKind::ProjectlessChats => self.visible_projectless_count = usize::MAX,
            PaginateKind::ProjectChats { path } => {
                self.project_chat_visible_counts.insert(path, usize::MAX);
            }
        }
        cx.notify();
    }

    fn select_chat(&mut self, index: usize, cx: &mut Context<Self>) {
        let parent = self.parent.clone();
        cx.defer(move |cx| {
            let _ = parent.update(cx, |parent, cx| parent.select_chat(index, cx));
        });
    }

    fn select_projectless_chat(&mut self, index: usize, cx: &mut Context<Self>) {
        let parent = self.parent.clone();
        cx.defer(move |cx| {
            let _ = parent.update(cx, |parent, cx| parent.select_projectless_chat(index, cx));
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
    visible_project_count: usize,
    projectless_chats: &[Chat],
    visible_projectless_count: usize,
    active_project: usize,
    active_chat: usize,
    active_projectless_chat: Option<usize>,
    active_project_path: Option<&str>,
    active_project_chats: &[Chat],
    visible_project_chat_count: usize,
    collapsed_projects: &HashSet<String>,
) -> Vec<SidebarRow<Project, Chat>> {
    let mut rows = Vec::with_capacity(projects.len() + projectless_chats.len() + 6);

    if !projects.is_empty() {
        rows.push(SidebarRow::SectionHeader { label: "Projects" });
        for (project_index, project) in projects.iter().take(visible_project_count).enumerate() {
            let selected = project_index == active_project && active_projectless_chat.is_none();
            let expanded = selected
                && active_project_path.is_some_and(|path| !collapsed_projects.contains(path));
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
            rows.extend(
                active_project_chats
                    .iter()
                    .take(visible_project_chat_count)
                    .enumerate()
                    .map(|(chat_index, chat)| SidebarRow::Chat {
                        project_index,
                        chat_index,
                        chat: chat.clone(),
                        selected: chat_index == active_chat,
                    }),
            );
            if active_project_chats.len() > visible_project_chat_count {
                rows.push(SidebarRow::ShowMore {
                    kind: PaginateKind::ProjectChats {
                        path: path.to_owned(),
                    },
                });
            }
        }
        if projects.len() > visible_project_count {
            rows.push(SidebarRow::ShowMore {
                kind: PaginateKind::Projects,
            });
        }
    }

    if !projectless_chats.is_empty() {
        rows.push(SidebarRow::SectionHeader { label: "Chats" });
        rows.extend(
            projectless_chats
                .iter()
                .take(visible_projectless_count)
                .enumerate()
                .map(|(chat_index, chat)| SidebarRow::ProjectlessChat {
                    chat_index,
                    chat: chat.clone(),
                    selected: active_projectless_chat == Some(chat_index),
                }),
        );
        if projectless_chats.len() > visible_projectless_count {
            rows.push(SidebarRow::ShowMore {
                kind: PaginateKind::ProjectlessChats,
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
        SidebarRow::SectionHeader { label } => div()
            .flex()
            .items_center()
            .size_full()
            .px_2()
            .text_xs()
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(cx.theme().muted_foreground)
            .child(label)
            .into_any_element(),
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
        SidebarRow::ProjectlessChat {
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
                format!("projectless-chat-{chat_index}"),
                title,
                subtitle,
                selected,
                cx.theme(),
            )
            .h_full()
            .on_click(move |_, _, cx| {
                let _ = sidebar.update(cx, |view, cx| view.select_projectless_chat(chat_index, cx));
            })
            .into_any_element()
        }
        SidebarRow::ShowMore { kind } => {
            let sidebar_more = sidebar.clone();
            let sidebar_all = sidebar.clone();
            let more_kind = kind.clone();
            let all_kind = kind.clone();
            let more = Button::new(format!("show-more-{kind:?}"))
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
                        .child("Show more"),
                )
                .on_click(move |_, _, cx| {
                    let kind = more_kind.clone();
                    let _ = sidebar_more.update(cx, |view, cx| view.show_more(kind, cx));
                });
            let all = Button::new(format!("show-all-{kind:?}"))
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
                        .child("Show all"),
                )
                .on_click(move |_, _, cx| {
                    let kind = all_kind.clone();
                    let _ = sidebar_all.update(cx, |view, cx| view.show_all(kind, cx));
                });
            div()
                .flex()
                .gap_2()
                .w_full()
                .h_full()
                .child(more)
                .child(all)
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
        let (
            projects,
            projectless_chats,
            active_project,
            active_chat,
            active_projectless_chat,
            threads_loaded,
        ) = {
            let state = self.state.read(cx);
            (
                state.projects.clone(),
                state.projectless_chats.clone(),
                state.active_project,
                state.active_chat,
                state.active_projectless_chat,
                state.threads_loaded,
            )
        };
        let (active_project_path, active_project_chats) = projects
            .get(active_project)
            .map(|project| {
                let project = project.read(cx);
                (Some(project.path.to_string()), project.chats.clone())
            })
            .unwrap_or_default();
        let visible_project_count = self.visible_project_count.min(projects.len());
        let visible_projectless_count = self.visible_projectless_count.min(projectless_chats.len());
        let visible_project_chat_count = self
            .project_chat_visible_counts
            .get(active_project_path.as_deref().unwrap_or_default())
            .copied()
            .unwrap_or(PAGE_SIZE)
            .min(active_project_chats.len());
        let rows = Arc::new(build_sidebar_rows(
            &projects,
            visible_project_count,
            &projectless_chats,
            visible_projectless_count,
            active_project,
            active_chat,
            active_projectless_chat,
            active_project_path.as_deref(),
            &active_project_chats,
            visible_project_chat_count,
            &self.collapsed_projects,
        ));
        let item_count_changed = self.list_state.item_count() != rows.len();
        if item_count_changed {
            self.list_state
                .reset_with_uniform_height(rows.len(), SIDEBAR_ROW_HEIGHT);
        }
        let active_project_row = rows.iter().position(|row| {
            matches!(
                row,
                SidebarRow::Project { project_index, .. } if *project_index == active_project
            )
        });
        if active_project_row.is_some()
            && (item_count_changed || self.list_active_project != Some(active_project))
        {
            self.list_state
                .scroll_to_reveal_item(active_project_row.expect("checked above"));
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
                    .child(if threads_loaded {
                        if rows.is_empty() {
                            div()
                                .size_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child("No threads yet"),
                                )
                                .into_any_element()
                        } else {
                            list(self.list_state.clone(), move |index, _, cx| {
                                rows.get(index)
                                    .cloned()
                                    .map(|row| render_sidebar_row(row, &sidebar, cx))
                                    .unwrap_or_else(|| div().into_any_element())
                            })
                            .size_full()
                            .into_any_element()
                        }
                    } else {
                        div()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap_2()
                            .child(Spinner::new().small().color(cx.theme().muted_foreground))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Loading threads…"),
                            )
                            .into_any_element()
                    })
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
            projects.len(),
            &[] as &[i32],
            0,
            1,
            2,
            None,
            Some("two"),
            &chats,
            5,
            &HashSet::new(),
        );

        assert_eq!(rows.len(), 10); // header + 3 projects + 5 chats + show-more.
        assert!(matches!(rows[0], SidebarRow::SectionHeader { .. }));
        assert!(matches!(
            rows[2],
            SidebarRow::Project {
                project_index: 1,
                expanded: true,
                ..
            }
        ));
        assert!(matches!(
            rows[5],
            SidebarRow::Chat {
                chat_index: 2,
                selected: true,
                ..
            }
        ));
        assert!(matches!(rows[8], SidebarRow::ShowMore { .. }));
    }

    #[test]
    fn projectless_chats_render_in_their_own_section() {
        let projects = vec!["one"];
        let projectless = vec![10, 11, 12];
        let rows = build_sidebar_rows(
            &projects,
            projects.len(),
            &projectless,
            projectless.len(),
            0,
            0,
            Some(1),
            Some("one"),
            &[] as &[i32],
            0,
            &HashSet::new(),
        );

        assert_eq!(rows.len(), 5); // projects header + project + chats header + 3 chats.
        assert!(matches!(rows[0], SidebarRow::SectionHeader { .. }));
        assert!(matches!(
            rows[1],
            SidebarRow::Project {
                selected: false,
                expanded: false,
                ..
            }
        ));
        assert!(matches!(rows[2], SidebarRow::SectionHeader { .. }));
        assert!(matches!(
            rows[3],
            SidebarRow::ProjectlessChat {
                chat_index: 0,
                selected: false,
                ..
            }
        ));
        assert!(matches!(
            rows[4],
            SidebarRow::ProjectlessChat {
                chat_index: 2,
                selected: true,
                ..
            }
        ));
    }

    #[test]
    fn pagination_adds_show_more_rows_for_both_sections() {
        let projects = (0..15).collect::<Vec<_>>();
        let projectless = (0..15).collect::<Vec<_>>();
        let rows = build_sidebar_rows(
            &projects,
            10,
            &projectless,
            10,
            0,
            0,
            None,
            Some("0"),
            &[] as &[i32],
            0,
            &HashSet::new(),
        );

        assert_eq!(rows.len(), 24); // header + 10 projects + show-more + header + 10 chats + show-more.
        let kinds = rows
            .iter()
            .filter_map(|row| match row {
                SidebarRow::ShowMore { kind } => Some(kind),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(kinds.len(), 2);
        assert!(matches!(kinds[0], PaginateKind::Projects));
        assert!(matches!(kinds[1], PaginateKind::ProjectlessChats));
    }

    #[test]
    fn expanded_show_all_and_collapse_change_only_the_flattened_rows() {
        let projects = vec!["one", "two"];
        let chats = (0..8).collect::<Vec<_>>();
        let rows = build_sidebar_rows(
            &projects,
            projects.len(),
            &[] as &[i32],
            0,
            0,
            0,
            None,
            Some("one"),
            &chats,
            chats.len(),
            &HashSet::new(),
        );
        assert_eq!(rows.len(), 11); // header + 2 projects + all 8 chats.
        assert!(
            !rows
                .iter()
                .any(|row| matches!(row, SidebarRow::ShowMore { .. }))
        );

        let collapsed = HashSet::from(["one".to_string()]);
        let rows = build_sidebar_rows(
            &projects,
            projects.len(),
            &[] as &[i32],
            0,
            0,
            0,
            None,
            Some("one"),
            &chats,
            chats.len(),
            &collapsed,
        );
        assert_eq!(rows.len(), 3); // header + 2 project rows.
        assert!(rows.iter().all(|row| matches!(
            row,
            SidebarRow::Project { .. } | SidebarRow::SectionHeader { .. }
        )));
    }
}
