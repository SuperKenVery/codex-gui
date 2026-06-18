use crate::app::CodexGui;
use crate::gui::{GuiState, widgets::chat_tree_item};
use gpui::{
    Context, Entity, IntoElement, MouseButton, ParentElement, Render, Styled, Subscription,
    WeakEntity, Window, WindowControlArea, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    v_flex,
};

pub struct Sidebar {
    parent: WeakEntity<CodexGui>,
    state: Entity<GuiState>,
    should_move_window: bool,
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
            _subscriptions: subscriptions,
        }
    }

    fn select_project(&mut self, index: usize, cx: &mut Context<Self>) {
        let parent = self.parent.clone();
        cx.defer(move |cx| {
            let _ = parent.update(cx, |parent, cx| parent.select_project(index, cx));
        });
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

        let project_tree =
            projects
                .iter()
                .enumerate()
                .fold(v_flex().gap_1(), |tree, (project_index, project)| {
                    let (name, path, chats) = {
                        let project = project.read(cx);
                        (
                            project.name.clone(),
                            project.path.clone(),
                            project.chats.clone(),
                        )
                    };
                    let project_selected = project_index == active_project;
                    let tree = tree.child(
                        Button::new(format!("project-{project_index}"))
                            .ghost()
                            .tooltip(path)
                            .selected(project_selected)
                            .with_size(px(0.))
                            .w_full()
                            .rounded_lg()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .w_full()
                                    .min_w_0()
                                    .py_1p5()
                                    .px_2()
                                    .child(
                                        Icon::new(if project_selected {
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
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.select_project(project_index, cx)
                            })),
                    );

                    if project_selected {
                        tree.child(v_flex().gap_1().children(chats.iter().enumerate().map(
                            |(chat_index, chat)| {
                                let (title, subtitle) = {
                                    let chat = chat.read(cx);
                                    (chat.title.clone(), chat.subtitle.clone())
                                };
                                chat_tree_item(
                                    format!("chat-{project_index}-{chat_index}"),
                                    title,
                                    subtitle,
                                    chat_index == active_chat,
                                    cx.theme(),
                                )
                                .on_click(cx.listener(
                                    move |view, _, _, cx| view.select_chat(chat_index, cx),
                                ))
                            },
                        )))
                    } else {
                        tree
                    }
                });

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
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(project_tree),
            )
    }
}
