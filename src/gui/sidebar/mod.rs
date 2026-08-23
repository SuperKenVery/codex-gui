use crate::app::CodexGui;
use crate::gui::GuiState;
use gpui::{
    Context, Entity, IntoElement, ListAlignment, ListState, MouseButton, ParentElement, Render,
    Styled, Subscription, WeakEntity, Window, WindowControlArea, div, list, prelude::*, px,
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

mod rows;

use rows::{PaginateKind, SidebarRow, build_sidebar_rows, render_sidebar_row};

const SIDEBAR_ROW_HEIGHT: gpui::Pixels = px(34.);
/// Vertical breathing room inside each row slot. Spacing between list rows must
/// come from padding, because the list measures border-box heights only and
/// margins on rows are ignored.
const SIDEBAR_ROW_GAP: gpui::Pixels = px(2.);
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
