use crate::app::CodexGui;
use crate::gui::{GuiState, ProjectState};
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
use std::collections::{HashMap, HashSet};
use std::ops::Range;

mod rows;

use rows::{PaginateKind, SidebarRowDisplayStatus, render_sidebar_row};

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
    /// Which projects are expanded? How many chats to show? etc.
    display_status: SidebarRowDisplayStatus,
    list_state: ListState,
    list_active_project: Option<usize>,
    observed_active_project: Option<Entity<ProjectState>>,
    _active_project_subscription: Option<Subscription>,
    _subscriptions: Vec<Subscription>,
}

impl Sidebar {
    pub fn new(
        parent: WeakEntity<CodexGui>,
        state: Entity<GuiState>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = vec![cx.observe(&state, |view, _, cx| {
            view.sync_rows_from_state(cx);
            cx.notify();
        })];
        let mut sidebar = Self {
            parent,
            state,
            should_move_window: false,
            collapsed_projects: HashSet::new(),
            visible_project_count: PAGE_SIZE,
            visible_projectless_count: PAGE_SIZE,
            project_chat_visible_counts: HashMap::new(),
            display_status: SidebarRowDisplayStatus::new(0, 0, 0, 0, 0, 0, None, None, 0, 0, true),
            list_state: ListState::new(0, ListAlignment::Top, SIDEBAR_LIST_OVERDRAW)
                .with_uniform_item_height(SIDEBAR_ROW_HEIGHT),
            list_active_project: None,
            observed_active_project: None,
            _active_project_subscription: None,
            _subscriptions: subscriptions,
        };
        sidebar.display_status = sidebar.rows_from_state(cx);
        sidebar.insert_rows(0, sidebar.display_status.len());
        sidebar.update_active_project_subscription(cx);
        sidebar
    }

    fn rows_from_state(&self, cx: &mut Context<Self>) -> SidebarRowDisplayStatus {
        let state = self.state.read(cx);
        let active_project = state.active_project;
        let active_projectless_chat = state.active_projectless_chat;
        let (active_project_path, active_project_chat_count) = state
            .projects
            .get(active_project)
            .map(|project| {
                let project = project.read(cx);
                (Some(project.path.to_string()), project.chats.len())
            })
            .unwrap_or_default();
        let visible_project_chat_count = self
            .project_chat_visible_counts
            .get(active_project_path.as_deref().unwrap_or_default())
            .copied()
            .unwrap_or(PAGE_SIZE);
        let active_project_collapsed = active_project_path
            .as_ref()
            .is_none_or(|path| self.collapsed_projects.contains(path));

        SidebarRowDisplayStatus::new(
            state.projects.len(),
            self.visible_project_count,
            state.projectless_chats.len(),
            self.visible_projectless_count,
            active_project,
            state.active_chat,
            active_projectless_chat,
            active_project_path,
            active_project_chat_count,
            visible_project_chat_count,
            active_project_collapsed,
        )
    }

    fn insert_rows(&self, index: usize, count: usize) {
        self.list_state.splice(index..index, count);
    }

    fn remove_rows(&self, range: Range<usize>) {
        self.list_state.splice(range, 0);
    }

    fn replace_rows(&self, range: Range<usize>, replacement_count: usize) {
        self.list_state.splice(range, replacement_count);
    }

    fn update_active_project_subscription(&mut self, cx: &mut Context<Self>) {
        let active_project = self.state.read(cx).active_project();
        if self.observed_active_project == active_project {
            return;
        }
        self.observed_active_project = active_project.clone();
        self._active_project_subscription = active_project.map(|project| {
            cx.observe(&project, |view, _, cx| {
                view.sync_rows_from_state(cx);
                cx.notify();
            })
        });
    }

    /// Reconciles model-driven structural changes without walking the flattened
    /// rows. View-driven changes use their more precise operations below.
    fn sync_rows_from_state(&mut self, cx: &mut Context<Self>) {
        let next = self.rows_from_state(cx);
        self.update_active_project_subscription(cx);
        if self.display_status == next {
            return;
        }

        let old_project_base_len = self.display_status.project_base_len();
        let new_project_base_len = next.project_base_len();
        let children_changed = !self.display_status.has_same_expanded_structure(&next);
        let project_base_changed = old_project_base_len != new_project_base_len;

        if children_changed || project_base_changed {
            if let Some(range) = self.display_status.expanded_children_range() {
                self.remove_rows(range);
            }
            if project_base_changed {
                self.replace_rows(0..old_project_base_len, new_project_base_len);
            }
            if let Some(range) = next.expanded_children_range() {
                self.insert_rows(range.start, range.len());
            }
        }

        let old_projectless_len = self.display_status.projectless_section_len();
        let new_projectless_len = next.projectless_section_len();
        if old_projectless_len != new_projectless_len {
            let section_start = next.project_section_len();
            if old_projectless_len == 0 || new_projectless_len == 0 {
                self.replace_rows(
                    section_start..section_start + old_projectless_len,
                    new_projectless_len,
                );
            } else {
                self.replace_rows(
                    section_start + 1..section_start + old_projectless_len,
                    new_projectless_len - 1,
                );
            }
        }

        self.display_status = next;
        debug_assert_eq!(self.list_state.item_count(), self.display_status.len());
    }

    fn apply_expansion_change(
        &self,
        before: &SidebarRowDisplayStatus,
        after: &SidebarRowDisplayStatus,
    ) {
        match (
            before.expanded_children_range(),
            after.expanded_children_range(),
        ) {
            (Some(old), Some(new)) => self.replace_rows(old, new.len()),
            (Some(old), None) => self.remove_rows(old),
            (None, Some(new)) => self.insert_rows(new.start, new.len()),
            (None, None) => {}
        }
    }

    fn paginate(&mut self, kind: PaginateKind, show_all: bool, cx: &mut Context<Self>) {
        let before = self.display_status.clone();
        let Some(pager_index) = before.pagination_pager_index(&kind) else {
            return;
        };
        let Some(old_body_len) = before.pagination_body_len(&kind) else {
            return;
        };

        match &kind {
            PaginateKind::Projects => {
                self.visible_project_count = if show_all {
                    usize::MAX
                } else {
                    self.visible_project_count.saturating_add(PAGE_SIZE)
                };
            }
            PaginateKind::ProjectlessChats => {
                self.visible_projectless_count = if show_all {
                    usize::MAX
                } else {
                    self.visible_projectless_count.saturating_add(PAGE_SIZE)
                };
            }
            PaginateKind::ProjectChats { path } => {
                let count = self
                    .project_chat_visible_counts
                    .entry(path.clone())
                    .or_insert(PAGE_SIZE);
                *count = if show_all {
                    usize::MAX
                } else {
                    count.saturating_add(PAGE_SIZE)
                };
            }
        }

        let after = self.rows_from_state(cx);
        let new_body_len = after
            .pagination_body_len(&kind)
            .expect("the paginated section remains present");
        let added_count = new_body_len.saturating_sub(old_body_len);
        if after.pagination_pager_index(&kind).is_some() {
            self.insert_rows(pager_index, added_count);
        } else {
            self.replace_rows(pager_index..pager_index + 1, added_count);
        }
        self.display_status = after;
        debug_assert_eq!(self.list_state.item_count(), self.display_status.len());
        cx.notify();
    }

    fn toggle_project(&mut self, index: usize, cx: &mut Context<Self>) {
        let state = self.state.read(cx);
        let Some(project) = state.projects.get(index) else {
            return;
        };
        let path = project.read(cx).path.to_string();
        let is_active = index == state.active_project && state.active_projectless_chat.is_none();

        if is_active {
            let before = self.display_status.clone();
            if !self.collapsed_projects.insert(path.clone()) {
                self.collapsed_projects.remove(&path);
            }
            let after = self.rows_from_state(cx);
            self.apply_expansion_change(&before, &after);
            self.display_status = after;
            debug_assert_eq!(self.list_state.item_count(), self.display_status.len());
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
        self.paginate(kind, false, cx);
    }

    fn show_all(&mut self, kind: PaginateKind, cx: &mut Context<Self>) {
        self.paginate(kind, true, cx);
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
        debug_assert_eq!(self.list_state.item_count(), self.display_status.len());
        let threads_loaded = self.state.read(cx).threads_loaded;
        let active_project = self.display_status.active_project();
        let active_project_row = self.display_status.active_project_row();
        if active_project_row.is_some() && self.list_active_project != Some(active_project) {
            self.list_state
                .scroll_to_reveal_item(active_project_row.expect("checked above"));
        }
        self.list_active_project = Some(active_project);
        let rows = self.display_status.clone();
        let state = self.state.clone();
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
                                let state = state.read(cx);
                                let active_project = state
                                    .projects
                                    .get(rows.active_project())
                                    .map(|project| project.read(cx));
                                let active_project_chats = active_project
                                    .as_ref()
                                    .map_or(&[][..], |project| project.chats.as_slice());
                                rows.row_at(
                                    index,
                                    &state.projects,
                                    &state.projectless_chats,
                                    active_project_chats,
                                )
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
