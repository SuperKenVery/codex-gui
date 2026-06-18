use crate::app::CodexGui;
use crate::gui::{
    GuiState,
    widgets::{command_button, nav_item, section_label},
};
use gpui::{
    Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, WeakEntity, Window,
    div, prelude::*, px,
};
use gpui_component::{ActiveTheme as _, button::ButtonVariants as _, v_flex};

pub struct Sidebar {
    parent: WeakEntity<CodexGui>,
    state: Entity<GuiState>,
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

    fn start_thread(&mut self, cx: &mut Context<Self>) {
        let parent = self.parent.clone();
        cx.defer(move |cx| {
            let _ = parent.update(cx, |parent, cx| parent.start_thread(cx));
        });
    }

    fn fork_chat(&mut self, cx: &mut Context<Self>) {
        let parent = self.parent.clone();
        cx.defer(move |cx| {
            let _ = parent.update(cx, |parent, cx| parent.fork_chat(cx));
        });
    }

    fn toggle_side_chat(&mut self, cx: &mut Context<Self>) {
        let parent = self.parent.clone();
        cx.defer(move |cx| {
            let _ = parent.update(cx, |parent, cx| parent.toggle_side_chat(cx));
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

        let project_items =
            v_flex()
                .gap_1()
                .children(projects.iter().enumerate().map(|(index, project)| {
                    let (name, path) = {
                        let project = project.read(cx);
                        (project.name.clone(), project.path.clone())
                    };
                    let selected = index == active_project;
                    nav_item(format!("project-{index}"), name, path, selected)
                        .on_click(cx.listener(move |view, _, _, cx| view.select_project(index, cx)))
                }));

        let chat_items = projects
            .get(active_project)
            .map(|project| {
                let chats = project.read(cx).chats.clone();
                v_flex()
                    .gap_1()
                    .children(chats.iter().enumerate().map(|(index, chat)| {
                        let (title, subtitle) = {
                            let chat = chat.read(cx);
                            (chat.title.clone(), chat.subtitle.clone())
                        };
                        let selected = index == active_chat;
                        nav_item(format!("chat-{index}"), title, subtitle, selected).on_click(
                            cx.listener(move |view, _, _, cx| view.select_chat(index, cx)),
                        )
                    }))
            })
            .unwrap_or_else(|| v_flex().gap_1());

        div()
            .w(px(286.))
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().sidebar)
            .p_3()
            .gap_3()
            .child(
                div()
                    .id("sidebar-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(section_label("Projects"))
                    .child(project_items)
                    .child(section_label("Codex Threads"))
                    .child(chat_items),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        command_button("start-thread", "New")
                            .primary()
                            .on_click(cx.listener(|view, _, _, cx| view.start_thread(cx))),
                    )
                    .child(
                        command_button("fork-chat", "Fork")
                            .on_click(cx.listener(|view, _, _, cx| view.fork_chat(cx))),
                    )
                    .child(
                        command_button("toggle-side-chat", "Side")
                            .on_click(cx.listener(|view, _, _, cx| view.toggle_side_chat(cx))),
                    ),
            )
    }
}
