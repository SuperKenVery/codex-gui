use std::{collections::HashSet, time::Duration};

use crate::gui::{
    ChatState, GuiState, Message, MessageState,
    widgets::{render_message, render_message_state, render_worked_summary},
};
use gpui::{
    AnyElement, Context, Entity, EntityId, IntoElement, ParentElement, Render, Styled,
    Subscription, Window, div, prelude::*,
};
use gpui_component::ActiveTheme as _;

pub struct ChatHistory {
    state: Entity<GuiState>,
    active_chat: Option<Entity<ChatState>>,
    _state_subscription: Subscription,
    chat_subscription: Option<Subscription>,
    expanded_turns: HashSet<EntityId>,
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
            expanded_turns: HashSet::new(),
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
            .map(|messages| self.render_message_list(messages, cx))
            .unwrap_or_else(|| {
                div()
                    .id("message-list")
                    .flex()
                    .flex_col()
                    .size_full()
                    .min_w_0()
                    .gap_3()
                    .overflow_x_hidden()
                    .overflow_y_scroll()
                    .child(render_message(
                        &Message::Commentary("Loading Codex threads from the app server.".into()),
                        cx.theme(),
                    ))
                    .into_any_element()
            })
    }
}

impl Render for MessageState {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        render_message_state(
            self,
            true,
            false,
            false,
            cx.theme(),
            cx.listener(|message, _, _, cx| {
                message.toggle_tools();
                cx.notify();
            }),
        )
    }
}

impl ChatHistory {
    fn render_message_list(
        &mut self,
        messages: Vec<Entity<MessageState>>,
        cx: &mut Context<ChatHistory>,
    ) -> AnyElement {
        let mut list = div()
            .id("message-list")
            .flex()
            .flex_col()
            .size_full()
            .min_w_0()
            .gap_2()
            .overflow_x_hidden()
            .overflow_y_scroll();

        let mut index = 0;
        while index < messages.len() {
            if is_user_message(&messages[index], cx) {
                let next_turn = next_user_index(&messages, index + 1, cx).unwrap_or(messages.len());
                if let Some(fold) = completed_turn_fold(&messages, index, next_turn, cx) {
                    let turn_id = messages[index].entity_id();
                    list = list.child(render_message_entity(
                        messages[index].clone(),
                        true,
                        false,
                        false,
                        cx,
                    ));

                    let summary = render_worked_summary(fold.duration, cx.theme())
                        .id(format!("worked-summary-{turn_id}"))
                        .on_click(cx.listener(move |history, _, _, cx| {
                            if !history.expanded_turns.remove(&turn_id) {
                                history.expanded_turns.insert(turn_id);
                            }
                            cx.notify();
                        }));
                    list = list.child(summary);

                    if self.expanded_turns.contains(&turn_id) {
                        for message in messages.iter().take(next_turn).skip(index + 1) {
                            list = list.child(render_message_entity(
                                message.clone(),
                                true,
                                false,
                                false,
                                cx,
                            ));
                        }
                    } else {
                        list = list.child(render_message_entity(
                            messages[fold.final_index].clone(),
                            true,
                            true,
                            false,
                            cx,
                        ));
                    }

                    index = next_turn;
                    continue;
                }
            }

            let active_tail = is_active_tool_tail(&messages, index, cx);
            list = list.child(render_message_entity(
                messages[index].clone(),
                true,
                false,
                active_tail,
                cx,
            ));
            index += 1;
        }

        list.into_any_element()
    }
}

fn render_message_entity(
    message: Entity<MessageState>,
    collapse_tools: bool,
    hide_tools: bool,
    active_tool_tail: bool,
    cx: &mut Context<ChatHistory>,
) -> impl IntoElement {
    let message_state = message.read(cx);
    render_message_state(
        message_state,
        collapse_tools,
        hide_tools,
        active_tool_tail,
        cx.theme(),
        move |_, _, cx| {
            let _ = message.update(cx, |message, cx| {
                message.toggle_tools();
                cx.notify();
            });
        },
    )
}

struct TurnFold {
    final_index: usize,
    duration: Duration,
}

fn completed_turn_fold(
    messages: &[Entity<MessageState>],
    user_index: usize,
    next_turn: usize,
    cx: &mut Context<ChatHistory>,
) -> Option<TurnFold> {
    let final_index = (user_index + 1..next_turn).rev().find(|index| {
        let message = messages[*index].read(cx);
        matches!(
            &message.message,
            Message::Assistant {
                body,
                state: crate::gui::StreamState::Complete,
                tools,
                ..
            } if !body.trim().is_empty()
                && tools.iter().all(|tool| matches!(tool.status, crate::gui::ToolStatus::Done))
        )
    })?;

    if next_turn == messages.len() && has_working_message(&messages[user_index + 1..next_turn], cx)
    {
        return None;
    }

    let has_progress = (user_index + 1..next_turn).any(|index| index != final_index);
    let final_has_tools = match &messages[final_index].read(cx).message {
        Message::Assistant { tools, .. } => !tools.is_empty(),
        _ => false,
    };
    if !has_progress && !final_has_tools {
        return None;
    }

    let first_progress = messages
        .get(user_index + 1)
        .map(|message| message.read(cx).created_at)?;
    let finished_at = messages[final_index].read(cx).updated_at;

    Some(TurnFold {
        final_index,
        duration: finished_at.saturating_duration_since(first_progress),
    })
}

fn has_working_message(messages: &[Entity<MessageState>], cx: &mut Context<ChatHistory>) -> bool {
    messages.iter().any(|message| {
        let message = message.read(cx);
        matches!(
            &message.message,
            Message::Assistant {
                state: crate::gui::StreamState::Streaming,
                ..
            }
        )
    })
}

fn is_active_tool_tail(
    messages: &[Entity<MessageState>],
    index: usize,
    cx: &mut Context<ChatHistory>,
) -> bool {
    if index + 1 != messages.len() {
        return false;
    }

    let message = messages[index].read(cx);
    matches!(
        &message.message,
        Message::Assistant {
            state: crate::gui::StreamState::Streaming,
            tools,
            ..
        } if !tools.is_empty()
    )
}

fn is_user_message(message: &Entity<MessageState>, cx: &mut Context<ChatHistory>) -> bool {
    matches!(&message.read(cx).message, Message::User(_))
}

fn next_user_index(
    messages: &[Entity<MessageState>],
    start: usize,
    cx: &mut Context<ChatHistory>,
) -> Option<usize> {
    (start..messages.len()).find(|index| is_user_message(&messages[*index], cx))
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
