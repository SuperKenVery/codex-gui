use crate::gui::{ChatState, GuiState, MessageState, widgets::render_message};
use crate::models::Message;
use gpui::{
    Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window, div,
    prelude::*,
};

pub struct ChatHistory {
    state: Entity<GuiState>,
    active_chat: Option<Entity<ChatState>>,
    _state_subscription: Subscription,
    chat_subscription: Option<Subscription>,
}

impl ChatHistory {
    pub fn new(state: Entity<GuiState>, cx: &mut Context<Self>) -> Self {
        let active_chat = active_chat_entity(&state, cx);
        let chat_subscription = active_chat
            .as_ref()
            .map(|chat| cx.observe(chat, |_, _, cx| cx.notify()));
        let state_subscription = cx.observe(&state, |history, _, cx| {
            history.update_active_chat_subscription(cx);
            cx.notify();
        });

        Self {
            state,
            active_chat,
            _state_subscription: state_subscription,
            chat_subscription,
        }
    }

    fn update_active_chat_subscription(&mut self, cx: &mut Context<Self>) {
        let active_chat = active_chat_entity(&self.state, cx);
        if self.active_chat == active_chat {
            return;
        }
        self.chat_subscription = active_chat
            .as_ref()
            .map(|chat| cx.observe(chat, |_, _, cx| cx.notify()));
        self.active_chat = active_chat;
    }
}

impl Render for ChatHistory {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let messages = self
            .active_chat
            .as_ref()
            .map(|chat| chat.read(cx).messages.clone());

        messages
            .map(|messages| {
                messages.into_iter().fold(
                    div()
                        .id("message-list")
                        .flex()
                        .flex_col()
                        .size_full()
                        .gap_3()
                        .overflow_y_scroll(),
                    |list, message| list.child(message),
                )
            })
            .unwrap_or_else(|| {
                div()
                    .id("message-list")
                    .flex()
                    .flex_col()
                    .size_full()
                    .gap_3()
                    .overflow_y_scroll()
                    .child(render_message(&Message::Commentary(
                        "Loading Codex threads from the app server.".into(),
                    )))
            })
    }
}

impl Render for MessageState {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        render_message(&self.message)
    }
}

fn active_chat_entity(
    state: &Entity<GuiState>,
    cx: &mut Context<ChatHistory>,
) -> Option<Entity<ChatState>> {
    let (project, active_chat) = {
        let state = state.read(cx);
        (state.active_project(), state.active_chat)
    };
    project.and_then(|project| {
        let chats = project.read(cx).chats.clone();
        chats.get(active_chat).cloned()
    })
}
