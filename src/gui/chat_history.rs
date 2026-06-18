use std::{collections::HashSet, time::Duration};

use crate::gui::{
    ChatState, GuiState, Message, MessageState,
    widgets::{render_message, render_message_state, render_worked_summary},
};
use gpui::{
    AnyElement, Context, Entity, EntityId, IntoElement, ListAlignment, ListState, ParentElement,
    Render, Styled, Subscription, Window, div, list, prelude::*, px,
};
use gpui_component::ActiveTheme as _;

pub struct ChatHistory {
    state: Entity<GuiState>,
    active_chat: Option<Entity<ChatState>>,
    _state_subscription: Subscription,
    chat_subscription: Option<Subscription>,
    message_subscriptions: Vec<Subscription>,
    expanded_turns: HashSet<EntityId>,
    list_state: ListState,
    row_keys: Vec<HistoryRowKey>,
}

impl ChatHistory {
    pub fn new(state: Entity<GuiState>, cx: &mut Context<Self>) -> Self {
        let active_chat = active_chat_entity(&state, cx);
        let chat_subscription = active_chat.as_ref().map(|chat| {
            cx.observe(chat, |history, _, cx| {
                history.list_state.remeasure();
                cx.notify()
            })
        });
        let state_subscription = cx.observe(&state, |history, _, cx| {
            history.update_active_chat_subscription(cx);
            cx.notify();
        });
        let message_subscriptions = active_chat
            .as_ref()
            .map(|chat| {
                let messages = chat.read(cx).messages.clone();
                subscribe_to_messages(&messages, cx)
            })
            .unwrap_or_default();

        Self {
            state,
            active_chat,
            _state_subscription: state_subscription,
            chat_subscription,
            message_subscriptions,
            expanded_turns: HashSet::new(),
            list_state: ListState::new(0, ListAlignment::Top, px(1000.)),
            row_keys: Vec::new(),
        }
    }

    fn update_active_chat_subscription(&mut self, cx: &mut Context<Self>) {
        let active_chat = active_chat_entity(&self.state, cx);
        if self.active_chat == active_chat {
            return;
        }
        self.chat_subscription = active_chat.as_ref().map(|chat| {
            cx.observe(chat, |history, _, cx| {
                history.list_state.remeasure();
                cx.notify()
            })
        });
        self.message_subscriptions = active_chat
            .as_ref()
            .map(|chat| {
                let messages = chat.read(cx).messages.clone();
                subscribe_to_messages(&messages, cx)
            })
            .unwrap_or_default();
        self.list_state.reset(0);
        self.row_keys.clear();
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
            self.collapse_tools,
            self.hide_tools,
            self.active_tool_tail,
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
        self.sync_message_subscriptions(&messages, cx);
        let rows = self.rows_from_messages(&messages, cx);
        self.sync_list_rows(&rows);

        let history = cx.entity().clone();
        let rows_for_render = rows.clone();
        list(
            self.list_state.clone(),
            move |index, _window, cx| match rows_for_render.get(index).cloned() {
                Some(HistoryRow::Message(message)) => message.into_any_element(),
                Some(HistoryRow::Summary { turn_id, duration }) => {
                    let history = history.clone();
                    render_worked_summary(duration, cx.theme())
                        .id(format!("worked-summary-{turn_id}"))
                        .on_click(move |_, _, cx| {
                            let _ = history.update(cx, |history, cx| {
                                if !history.expanded_turns.remove(&turn_id) {
                                    history.expanded_turns.insert(turn_id);
                                }
                                history.rebuild_rows(cx);
                                cx.notify();
                            });
                        })
                        .into_any_element()
                }
                None => div().into_any_element(),
            },
        )
        .size_full()
        .min_w_0()
        .into_any_element()
    }

    fn rows_from_messages(
        &self,
        messages: &[Entity<MessageState>],
        cx: &mut Context<ChatHistory>,
    ) -> Vec<HistoryRow> {
        let mut rows = Vec::new();
        let mut index = 0;
        while index < messages.len() {
            if is_user_message(&messages[index], cx) {
                let next_turn = next_user_index(messages, index + 1, cx).unwrap_or(messages.len());
                if let Some(fold) = completed_turn_fold(messages, index, next_turn, cx) {
                    let turn_id = messages[index].entity_id();
                    configure_message(&messages[index], true, false, false, cx);
                    rows.push(HistoryRow::Message(messages[index].clone()));
                    rows.push(HistoryRow::Summary {
                        turn_id,
                        duration: fold.duration,
                    });

                    if self.expanded_turns.contains(&turn_id) {
                        for message in messages.iter().take(next_turn).skip(index + 1) {
                            configure_message(message, true, false, false, cx);
                            rows.push(HistoryRow::Message(message.clone()));
                        }
                    } else {
                        configure_message(&messages[fold.final_index], true, true, false, cx);
                        rows.push(HistoryRow::Message(messages[fold.final_index].clone()));
                    }

                    index = next_turn;
                    continue;
                }
            }

            let active_tail = is_active_tool_tail(messages, index, cx);
            configure_message(&messages[index], true, false, active_tail, cx);
            rows.push(HistoryRow::Message(messages[index].clone()));
            index += 1;
        }
        rows
    }

    fn sync_list_rows(&mut self, rows: &[HistoryRow]) {
        let next_keys = rows.iter().map(HistoryRow::key).collect::<Vec<_>>();
        if next_keys == self.row_keys {
            return;
        }

        let old_len = self.row_keys.len();
        let new_len = next_keys.len();
        let prefix = self
            .row_keys
            .iter()
            .zip(next_keys.iter())
            .take_while(|(old, new)| old == new)
            .count();
        let suffix = self.row_keys[prefix..]
            .iter()
            .rev()
            .zip(next_keys[prefix..].iter().rev())
            .take_while(|(old, new)| old == new)
            .count();
        let old_end = old_len.saturating_sub(suffix);
        let new_end = new_len.saturating_sub(suffix);
        self.list_state.splice(prefix..old_end, new_end - prefix);
        self.row_keys = next_keys;
    }

    fn sync_message_subscriptions(
        &mut self,
        messages: &[Entity<MessageState>],
        cx: &mut Context<ChatHistory>,
    ) {
        if self.message_subscriptions.len() == messages.len() {
            return;
        }
        self.message_subscriptions = subscribe_to_messages(messages, cx);
    }

    fn rebuild_rows(&mut self, cx: &mut Context<ChatHistory>) {
        let Some(chat) = &self.active_chat else {
            self.sync_list_rows(&[]);
            return;
        };
        let messages = chat.read(cx).messages.clone();
        let rows = self.rows_from_messages(&messages, cx);
        self.sync_list_rows(&rows);
    }
}

#[derive(Clone)]
enum HistoryRow {
    Message(Entity<MessageState>),
    Summary {
        turn_id: EntityId,
        duration: Duration,
    },
}

impl HistoryRow {
    fn key(&self) -> HistoryRowKey {
        match self {
            HistoryRow::Message(message) => HistoryRowKey::Message(message.entity_id()),
            HistoryRow::Summary { turn_id, .. } => HistoryRowKey::Summary(*turn_id),
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum HistoryRowKey {
    Message(EntityId),
    Summary(EntityId),
}

fn configure_message(
    message: &Entity<MessageState>,
    collapse_tools: bool,
    hide_tools: bool,
    active_tool_tail: bool,
    cx: &mut Context<ChatHistory>,
) {
    message.update(cx, |message, _| {
        message.set_render_options(collapse_tools, hide_tools, active_tool_tail);
    });
}

fn subscribe_to_messages(
    messages: &[Entity<MessageState>],
    cx: &mut Context<ChatHistory>,
) -> Vec<Subscription> {
    messages
        .iter()
        .map(|message| {
            cx.observe(message, |history, _, cx| {
                history.list_state.remeasure();
                cx.notify();
            })
        })
        .collect()
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
