use super::Sidebar;
use super::motion::ProjectFoldAnimation;
use super::rows::PaginateKind;
use gpui::Context;

impl Sidebar {
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
                    self.visible_project_count.saturating_add(super::PAGE_SIZE)
                };
            }
            PaginateKind::ProjectlessChats => {
                self.visible_projectless_count = if show_all {
                    usize::MAX
                } else {
                    self.visible_projectless_count
                        .saturating_add(super::PAGE_SIZE)
                };
            }
            PaginateKind::ProjectChats { path } => {
                let count = self
                    .project_chat_visible_counts
                    .entry(path.clone())
                    .or_insert(super::PAGE_SIZE);
                *count = if show_all {
                    usize::MAX
                } else {
                    count.saturating_add(super::PAGE_SIZE)
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
            self.list_state
                .splice(pager_index..pager_index + 1, added_count);
        }
        self.display_status = after;
        debug_assert_eq!(self.list_state.item_count(), self.display_status.len());
        cx.notify();
    }

    pub(super) fn toggle_project(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(generation) = self
            .departing_project_fold_animation
            .as_ref()
            .map(|animation| animation.generation)
        {
            self.finish_departing_project_fold_animation(generation, cx);
        }

        let (path, is_active, active_project_path) = {
            let state = self.state.read(cx);
            let Some(project) = state.projects.get(index) else {
                return;
            };
            (
                project.read(cx).path.to_string(),
                index == state.active_project && state.active_projectless_chat.is_none(),
                state
                    .active_project()
                    .map(|project| project.read(cx).path.to_string()),
            )
        };

        if is_active {
            let targeting_expanded = self
                .project_fold_animation
                .as_ref()
                .filter(|animation| animation.project_path == path)
                .map_or_else(
                    || !self.collapsed_projects.contains(&path),
                    ProjectFoldAnimation::is_expanding,
                );
            let from = self
                .project_fold_animation
                .as_ref()
                .filter(|animation| animation.project_path == path)
                .map(ProjectFoldAnimation::current)
                .unwrap_or(if targeting_expanded { 1. } else { 0. });

            if targeting_expanded {
                self.start_project_fold_animation(path, from, false, cx);
            } else {
                self.collapsed_projects.remove(&path);
                let before = self.display_status.clone();
                let after = self.rows_from_state(cx);
                self.apply_expansion_change(&before, &after);
                self.display_status = after;
                debug_assert_eq!(self.list_state.item_count(), self.display_status.len());
                self.start_project_fold_animation(path, from, true, cx);
            }
            return;
        }

        self.collapsed_projects.remove(&path);
        if let Some(active_project_path) = active_project_path
            && self.display_status.expanded_project_path() == Some(active_project_path.as_str())
            && !cx.reduce_motion()
        {
            let from = self
                .project_fold_animation
                .take()
                .as_ref()
                .filter(|animation| animation.project_path == active_project_path)
                .map_or(1., ProjectFoldAnimation::current);
            self.start_departing_project_fold_animation(active_project_path, from, cx);
            self.select_and_expand_project(path, cx);
            return;
        }

        self.select_and_expand_project(path, cx);
    }

    pub(super) fn select_and_expand_project(&mut self, path: String, cx: &mut Context<Self>) {
        let index = self
            .state
            .read(cx)
            .projects
            .iter()
            .position(|project| project.read(cx).path.as_ref() == path.as_str());
        let Some(index) = index else {
            return;
        };

        self.collapsed_projects.remove(&path);
        self.pending_project_expansion = Some(path);
        let parent = self.parent.clone();
        cx.defer(move |cx| {
            let _ = parent.update(cx, |parent, cx| parent.select_project(index, cx));
        });
    }

    pub(super) fn start_project_expansion(
        &mut self,
        project_path: String,
        from: f32,
        cx: &mut Context<Self>,
    ) {
        self.start_project_fold_animation(project_path, from, true, cx);
    }

    fn start_project_fold_animation(
        &mut self,
        project_path: String,
        from: f32,
        expanding: bool,
        cx: &mut Context<Self>,
    ) {
        self.project_fold_generation = self.project_fold_generation.wrapping_add(1);
        let animation =
            ProjectFoldAnimation::new(project_path, self.project_fold_generation, from, expanding);
        let generation = animation.generation;
        let duration = animation.duration;
        self.project_fold_animation = Some(animation);

        if cx.reduce_motion() || duration.is_zero() {
            self.finish_project_fold_animation(generation, cx);
            return;
        }

        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(duration).await;
            let _ = this.update(cx, |view, cx| {
                view.finish_project_fold_animation(generation, cx)
            });
        })
        .detach();
        cx.notify();
    }

    fn start_departing_project_fold_animation(
        &mut self,
        project_path: String,
        from: f32,
        cx: &mut Context<Self>,
    ) {
        self.project_fold_generation = self.project_fold_generation.wrapping_add(1);
        let animation =
            ProjectFoldAnimation::new(project_path, self.project_fold_generation, from, false);
        let generation = animation.generation;
        let duration = animation.duration;
        self.departing_project_fold_animation = Some(animation);

        if duration.is_zero() {
            self.finish_departing_project_fold_animation(generation, cx);
            return;
        }

        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(duration).await;
            let _ = this.update(cx, |view, cx| {
                view.finish_departing_project_fold_animation(generation, cx)
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn show_more(&mut self, kind: PaginateKind, cx: &mut Context<Self>) {
        self.paginate(kind, false, cx);
    }

    pub(super) fn show_all(&mut self, kind: PaginateKind, cx: &mut Context<Self>) {
        self.paginate(kind, true, cx);
    }

    pub(super) fn select_chat(&mut self, index: usize, cx: &mut Context<Self>) {
        let parent = self.parent.clone();
        cx.defer(move |cx| {
            let _ = parent.update(cx, |parent, cx| parent.select_chat(index, cx));
        });
    }

    pub(super) fn select_projectless_chat(&mut self, index: usize, cx: &mut Context<Self>) {
        let parent = self.parent.clone();
        cx.defer(move |cx| {
            let _ = parent.update(cx, |parent, cx| parent.select_projectless_chat(index, cx));
        });
    }

    pub(super) fn open_new_chat(&mut self, cx: &mut Context<Self>) {
        let parent = self.parent.clone();
        cx.defer(move |cx| {
            let _ = parent.update(cx, |parent, cx| parent.open_new_chat(cx));
        });
    }
}
