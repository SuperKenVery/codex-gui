use super::Sidebar;
use super::motion::project_child_reveal_progress;
use super::rows::{SidebarRow, render_sidebar_row};
use gpui::{
    Animation, AnimationExt as _, Context, IntoElement, MouseButton, ParentElement, Render, Styled,
    Window, WindowControlArea, div, list, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    scroll::ScrollableElement as _,
    spinner::Spinner,
};

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
        let fold_animation = self.project_fold_animation.clone();
        let departing_fold_animation = self.departing_project_fold_animation.clone();
        let animated_children = fold_animation
            .as_ref()
            .and_then(|_| rows.expanded_children_range());
        let departing_animated_children = departing_fold_animation
            .as_ref()
            .and_then(|_| rows.departing_children_range());

        div()
            .w(px(286.))
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(cx.theme().border.opacity(0.35))
            .bg(cx.theme().sidebar.opacity(0.5))
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
                            let row_animation = fold_animation.clone();
                            let departing_row_animation = departing_fold_animation.clone();
                            let sidebar_list =
                                list(self.list_state.clone(), move |index, _, cx| {
                                    let state = state.read(cx);
                                    let active_project = state
                                        .projects
                                        .get(rows.active_project())
                                        .map(|project| project.read(cx));
                                    let active_project_chats = active_project
                                        .as_ref()
                                        .map_or(&[][..], |project| project.chats.as_slice());
                                    let departing_project = rows
                                        .departing_project_index()
                                        .and_then(|project_index| state.projects.get(project_index))
                                        .map(|project| project.read(cx));
                                    let departing_project_chats = departing_project
                                        .as_ref()
                                        .map_or(&[][..], |project| project.chats.as_slice());
                                    let mut row = rows.row_at(
                                        index,
                                        &state.projects,
                                        &state.projectless_chats,
                                        active_project_chats,
                                        departing_project_chats,
                                    );
                                    if let Some(SidebarRow::Project {
                                        project, expanded, ..
                                    }) = row.as_mut()
                                    {
                                        let path = project.read(cx).path.to_string();
                                        if let Some(animation) = row_animation
                                            .as_ref()
                                            .filter(|animation| animation.project_path == path)
                                            .or_else(|| {
                                                departing_row_animation.as_ref().filter(
                                                    |animation| animation.project_path == path,
                                                )
                                            })
                                        {
                                            *expanded = animation.is_expanding();
                                        }
                                    }
                                    let reveal = animated_children
                                        .as_ref()
                                        .filter(|range| range.contains(&index))
                                        .map(|range| {
                                            let progress = row_animation
                                                .as_ref()
                                                .expect("animated range requires fold animation")
                                                .current();
                                            project_child_reveal_progress(
                                                progress,
                                                index - range.start,
                                                range.len(),
                                            )
                                        })
                                        .or_else(|| {
                                            departing_animated_children
                                                .as_ref()
                                                .filter(|range| range.contains(&index))
                                                .map(|range| {
                                                    let progress = departing_row_animation
                                                        .as_ref()
                                                        .expect("animated range requires fold animation")
                                                        .current();
                                                    project_child_reveal_progress(
                                                        progress,
                                                        index - range.start,
                                                        range.len(),
                                                    )
                                                })
                                        });
                                    row.map(|row| render_sidebar_row(row, reveal, &sidebar, cx))
                                        .unwrap_or_else(|| div().into_any_element())
                                })
                                .size_full();

                            match (fold_animation, departing_fold_animation) {
                                (Some(animation), Some(departing_animation)) => {
                                    let generation = animation.generation;
                                    let duration = animation.duration;
                                    let departing_generation = departing_animation.generation;
                                    let departing_duration = departing_animation.duration;
                                    sidebar_list
                                        .with_animation(
                                            format!("project-fold-{generation}"),
                                            Animation::new(duration)
                                                .with_easing(|x| 1.0 - (x - 1.0).powi(5).abs()),
                                            move |list, delta| {
                                                animation.update(delta);
                                                list
                                            },
                                        )
                                        .with_animation(
                                            format!("project-fold-{departing_generation}"),
                                            Animation::new(departing_duration)
                                                .with_easing(|x| 1.0 - (x - 1.0).powi(5).abs()),
                                            move |list, delta| {
                                                departing_animation.update(delta);
                                                list
                                            },
                                        )
                                        .into_any_element()
                                }
                                (Some(animation), None) | (None, Some(animation)) => {
                                    let generation = animation.generation;
                                    let duration = animation.duration;
                                    sidebar_list
                                        .with_animation(
                                            format!("project-fold-{generation}"),
                                            Animation::new(duration)
                                                .with_easing(|x| 1.0 - (x - 1.0).powi(5).abs()),
                                            move |list, delta| {
                                                animation.update(delta);
                                                list
                                            },
                                        )
                                        .into_any_element()
                                }
                                (None, None) => sidebar_list.into_any_element(),
                            }
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
