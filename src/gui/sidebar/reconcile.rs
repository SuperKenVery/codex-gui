use super::{PAGE_SIZE, Sidebar, SidebarRowDisplayStatus};
use crate::gui::ProjectState;
use gpui::{Context, Entity};
use std::ops::Range;

impl Sidebar {
    pub(super) fn rows_from_state(&self, cx: &mut Context<Self>) -> SidebarRowDisplayStatus {
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

    pub(super) fn insert_rows(&self, index: usize, count: usize) {
        self.list_state.splice(index..index, count);
    }

    fn remove_rows(&self, range: Range<usize>) {
        self.list_state.splice(range, 0);
    }

    fn replace_rows(&self, range: Range<usize>, replacement_count: usize) {
        self.list_state.splice(range, replacement_count);
    }

    pub(super) fn update_active_project_subscription(&mut self, cx: &mut Context<Self>) {
        let active_project = self.state.read(cx).active_project();
        if self.observed_active_project == active_project {
            return;
        }
        self.observed_active_project = active_project.clone();
        self._active_project_subscription = active_project.map(|project: Entity<ProjectState>| {
            cx.observe(&project, |view, _, cx| {
                view.sync_rows_from_state(cx);
                cx.notify();
            })
        });
    }

    /// Reconciles model-driven structural changes without walking the flattened
    /// rows. View-driven changes use their more precise operations instead.
    pub(super) fn sync_rows_from_state(&mut self, cx: &mut Context<Self>) {
        self.update_active_project_subscription(cx);
        let active_project_path = self
            .state
            .read(cx)
            .active_project()
            .map(|project| project.read(cx).path.to_string());
        if self
            .project_fold_animation
            .as_ref()
            .is_some_and(|animation| Some(&animation.project_path) != active_project_path.as_ref())
        {
            if let Some(animation) = self.project_fold_animation.take()
                && animation.is_collapsing()
            {
                self.collapsed_projects.insert(animation.project_path);
            }
        }

        let mut next = self.rows_from_state(cx);
        let retaining_departing_project = self
            .departing_project_fold_animation
            .as_ref()
            .filter(|animation| Some(&animation.project_path) != active_project_path.as_ref())
            .is_some_and(|animation| {
                next.retain_departing_project_from(&self.display_status, &animation.project_path)
            });
        if self.display_status == next {
            return;
        }

        let animate_new_project = active_project_path.as_ref().is_some_and(|path| {
            self.pending_project_expansion.as_ref() == Some(path)
                && next
                    .expanded_children_range()
                    .is_some_and(|range| !range.is_empty())
        });

        let old_project_base_len = self.display_status.project_base_len();
        let new_project_base_len = next.project_base_len();
        let children_changed = !self.display_status.has_same_expanded_structure(&next);
        let project_base_changed = old_project_base_len != new_project_base_len;
        let transitioning_between_projects =
            retaining_departing_project || self.display_status.departing_project_path().is_some();

        if children_changed || project_base_changed {
            if retaining_departing_project
                && self.display_status.departing_project_path().is_none()
                && !project_base_changed
            {
                if let Some(range) = next.expanded_children_range() {
                    self.insert_rows(range.start, range.len());
                }
            } else if transitioning_between_projects {
                self.replace_rows(
                    0..self.display_status.project_section_len(),
                    next.project_section_len(),
                );
            } else {
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
        if animate_new_project {
            let path = self
                .pending_project_expansion
                .take()
                .expect("checked pending project expansion above");
            self.start_project_expansion(path, 0., cx);
        } else if active_project_path
            .as_ref()
            .is_some_and(|path| self.pending_project_expansion.as_ref() == Some(path))
        {
            self.pending_project_expansion = None;
        }
    }

    pub(super) fn apply_expansion_change(
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

    pub(super) fn finish_project_fold_animation(
        &mut self,
        generation: u64,
        cx: &mut Context<Self>,
    ) {
        let Some(animation) = self
            .project_fold_animation
            .as_ref()
            .filter(|animation| animation.generation == generation)
            .cloned()
        else {
            return;
        };

        self.project_fold_animation = None;
        if animation.is_collapsing() {
            self.collapsed_projects.insert(animation.project_path);
            let before = self.display_status.clone();
            let after = self.rows_from_state(cx);
            self.apply_expansion_change(&before, &after);
            self.display_status = after;
            debug_assert_eq!(self.list_state.item_count(), self.display_status.len());
        }
        cx.notify();
    }

    pub(super) fn finish_departing_project_fold_animation(
        &mut self,
        generation: u64,
        cx: &mut Context<Self>,
    ) {
        let Some(animation) = self
            .departing_project_fold_animation
            .as_ref()
            .filter(|animation| animation.generation == generation)
            .cloned()
        else {
            return;
        };

        self.departing_project_fold_animation = None;
        self.collapsed_projects.insert(animation.project_path);
        let before = self.display_status.clone();
        let after = self.rows_from_state(cx);
        if let Some(range) = before.departing_children_range() {
            self.remove_rows(range);
        } else {
            self.apply_expansion_change(&before, &after);
        }
        self.display_status = after;
        debug_assert_eq!(self.list_state.item_count(), self.display_status.len());
        cx.notify();
    }
}
