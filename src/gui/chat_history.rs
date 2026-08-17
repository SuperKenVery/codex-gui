use std::{
    collections::HashSet,
    sync::{Arc, RwLock},
    time::Duration,
};

use crate::gui::{
    ChatState, GuiState, HistoryKey, MessageState, StreamState,
    transcript::{
        TranscriptBlockStore, TranscriptBlockTarget, TranscriptDocument, TranscriptPlugin,
    },
    widgets::{
        render_assistant_header, render_notice, render_tool_group, render_user_message,
        render_worked_summary, user_input_text,
    },
};
use codex_app_server_protocol::ThreadItem;
use codex_protocol::models::MessagePhase;
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, EntityId, IntoElement, ParentElement,
    Render, Styled, Subscription, WeakEntity, Window, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _,
    text::{MarkdownExtensions, TextView, TextViewState},
};

pub struct ChatHistory {
    state: Entity<GuiState>,
    active_chat: Option<Entity<ChatState>>,
    _state_subscription: Subscription,
    chat_subscription: Option<Subscription>,
    message_subscriptions: Vec<Subscription>,
    subscribed_message_ids: Vec<EntityId>,
    expanded_turns: HashSet<EntityId>,
    transcript: Entity<TextViewState>,
    transcript_extensions: MarkdownExtensions,
    transcript_blocks: TranscriptBlockStore,
    transcript_source: String,
    transcript_chat_id: Option<String>,
}

impl ChatHistory {
    pub fn new(state: Entity<GuiState>, cx: &mut Context<Self>) -> Self {
        let active_chat = active_chat_entity(&state, cx);
        let chat_subscription = subscribe_to_chat(active_chat.as_ref(), cx);
        let state_subscription = cx.observe(&state, |history, _, cx| {
            history.update_active_chat_subscription(cx);
            cx.notify();
        });
        let transcript = cx.new(|cx| TextViewState::markdown("", cx));
        let transcript_blocks = Arc::new(RwLock::new(Default::default()));
        let transcript_extensions =
            TranscriptPlugin::new(cx.entity().downgrade(), transcript_blocks.clone()).extensions();

        let mut history = Self {
            state,
            active_chat,
            _state_subscription: state_subscription,
            chat_subscription,
            message_subscriptions: Vec::new(),
            subscribed_message_ids: Vec::new(),
            expanded_turns: HashSet::new(),
            transcript,
            transcript_extensions,
            transcript_blocks,
            transcript_source: String::new(),
            transcript_chat_id: None,
        };
        history.rebuild_transcript(cx);
        history
    }

    fn update_active_chat_subscription(&mut self, cx: &mut Context<Self>) {
        let active_chat = active_chat_entity(&self.state, cx);
        if self.active_chat == active_chat {
            return;
        }

        self.chat_subscription = subscribe_to_chat(active_chat.as_ref(), cx);
        self.message_subscriptions.clear();
        self.subscribed_message_ids.clear();
        self.active_chat = active_chat;
        self.transcript = cx.new(|cx| TextViewState::markdown("", cx));
        self.transcript_source.clear();
        self.transcript_chat_id = None;
        self.rebuild_transcript(cx);
    }

    fn sync_message_subscriptions(
        &mut self,
        messages: &[Entity<MessageState>],
        cx: &mut Context<Self>,
    ) {
        let message_ids = messages.iter().map(Entity::entity_id).collect::<Vec<_>>();
        if message_ids == self.subscribed_message_ids {
            return;
        }

        self.message_subscriptions = messages
            .iter()
            .map(|message| {
                cx.observe(message, |history, _, cx| {
                    history.rebuild_transcript(cx);
                    cx.notify();
                })
            })
            .collect();
        self.subscribed_message_ids = message_ids;
    }

    fn rebuild_transcript(&mut self, cx: &mut Context<Self>) {
        let Some(chat) = self.active_chat.clone() else {
            self.sync_transcript(None, TranscriptDocument::new(), cx);
            return;
        };
        let messages = chat.read(cx).messages.clone();
        self.sync_message_subscriptions(&messages, cx);
        let rows = self.rows_from_messages(&chat, &messages, cx);
        let document = self.document_from_rows(&chat, &rows, cx);
        let chat_id = chat.read(cx).id.clone();
        self.sync_transcript(Some(chat_id), document, cx);
    }

    fn sync_transcript(
        &mut self,
        chat_id: Option<String>,
        document: TranscriptDocument,
        cx: &mut Context<Self>,
    ) {
        if let Ok(mut blocks) = self.transcript_blocks.write() {
            *blocks = document.blocks;
        }

        let same_chat = self.transcript_chat_id == chat_id;
        let old_source = std::mem::replace(&mut self.transcript_source, document.source);
        self.transcript_chat_id = chat_id;

        if same_chat && old_source == self.transcript_source {
            return;
        }

        let appended = if same_chat && !old_source.is_empty() {
            self.transcript_source
                .strip_prefix(&old_source)
                .map(str::to_string)
        } else {
            None
        };
        let source = self.transcript_source.clone();
        self.transcript.update(cx, |state, cx| {
            if let Some(delta) = appended {
                state.push_str(&delta, cx);
            } else {
                state.set_text(&source, cx);
            }
        });
    }

    fn rows_from_messages(
        &self,
        chat: &Entity<ChatState>,
        messages: &[Entity<MessageState>],
        cx: &mut Context<Self>,
    ) -> Vec<HistoryRow> {
        let mut rows = Vec::new();
        let mut index = 0;
        while index < messages.len() {
            if is_user_message(chat, &messages[index], cx) {
                let next_turn =
                    next_user_index(chat, messages, index + 1, cx).unwrap_or(messages.len());
                if let Some(fold) = completed_turn_fold(chat, messages, index, next_turn, cx) {
                    let turn_id = messages[index].entity_id();
                    rows.push(HistoryRow::message(messages[index].clone()));
                    rows.push(HistoryRow::Summary {
                        turn_id,
                        duration: fold.duration,
                        expanded: self.expanded_turns.contains(&turn_id),
                    });

                    if self.expanded_turns.contains(&turn_id) {
                        rows.extend(
                            messages[index + 1..next_turn]
                                .iter()
                                .cloned()
                                .map(HistoryRow::message),
                        );
                    } else {
                        rows.push(HistoryRow::Message {
                            message: messages[fold.final_index].clone(),
                            options: MessageRenderOptions {
                                hide_tools: true,
                                ..Default::default()
                            },
                        });
                    }

                    index = next_turn;
                    continue;
                }
            }

            rows.push(HistoryRow::Message {
                message: messages[index].clone(),
                options: MessageRenderOptions {
                    active_tool_tail: is_active_tool_tail(chat, messages, index, cx),
                    ..Default::default()
                },
            });
            index += 1;
        }
        rows
    }

    fn document_from_rows(
        &self,
        chat: &Entity<ChatState>,
        rows: &[HistoryRow],
        cx: &mut Context<Self>,
    ) -> TranscriptDocument {
        let mut document = TranscriptDocument::new();
        for row in rows {
            match row {
                HistoryRow::Summary {
                    turn_id,
                    duration,
                    expanded,
                } => document.push_block(TranscriptBlockTarget::WorkedSummary {
                    turn_id: *turn_id,
                    duration: *duration,
                    expanded: *expanded,
                }),
                HistoryRow::Message { message, options } => {
                    let message = message.read(cx);
                    let chat = chat.read(cx);
                    match chat.item_for_state(message) {
                        Some(ThreadItem::UserMessage { .. }) => {
                            document.push_block(TranscriptBlockTarget::User {
                                key: message.key.clone(),
                            });
                        }
                        Some(ThreadItem::AgentMessage { text, phase, .. }) => {
                            let label = match (phase.as_ref(), message.stream_state) {
                                (Some(MessagePhase::Commentary), _) => "",
                                (_, StreamState::Complete) => "Codex",
                                (_, StreamState::Streaming) => "Codex is working",
                            };
                            if !label.is_empty() {
                                document.push_block(TranscriptBlockTarget::AssistantHeader {
                                    key: message.key.clone(),
                                    label,
                                });
                            }
                            document.push_markdown(text);
                            if !options.hide_tools && !chat.tools_for_state(message).is_empty() {
                                document.push_block(TranscriptBlockTarget::Tools {
                                    key: message.key.clone(),
                                    collapse: options.collapse_tools,
                                    active_tail: options.active_tool_tail,
                                });
                            }
                        }
                        Some(item) if is_tool_item(item) => {
                            if !options.hide_tools && !chat.tools_for_state(message).is_empty() {
                                document.push_block(TranscriptBlockTarget::Tools {
                                    key: message.key.clone(),
                                    collapse: options.collapse_tools,
                                    active_tail: options.active_tool_tail,
                                });
                            }
                        }
                        None => document.push_markdown(&message.rendered_body),
                        Some(_) => {}
                    }
                }
            }
        }
        document
    }
}

impl Render for ChatHistory {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.active_chat.is_none() {
            return div()
                .id("message-list")
                .flex()
                .flex_col()
                .size_full()
                .min_w_0()
                .gap_3()
                .overflow_x_hidden()
                .overflow_y_scroll()
                .child(render_notice(
                    "Loading Codex threads from the app server.",
                    cx.theme(),
                ))
                .into_any_element();
        }

        div()
            .id("message-list")
            .size_full()
            .min_w_0()
            .overflow_hidden()
            .child(
                TextView::new(&self.transcript)
                    .markdown_extensions(self.transcript_extensions.clone())
                    .selectable(true)
                    .scrollable(true)
                    .size_full()
                    .min_w_0()
                    .text_sm()
                    .line_height(px(22.))
                    .text_color(cx.theme().foreground),
            )
            .into_any_element()
    }
}

#[derive(Clone, Copy)]
struct MessageRenderOptions {
    collapse_tools: bool,
    hide_tools: bool,
    active_tool_tail: bool,
}

impl Default for MessageRenderOptions {
    fn default() -> Self {
        Self {
            collapse_tools: true,
            hide_tools: false,
            active_tool_tail: false,
        }
    }
}

#[derive(Clone)]
enum HistoryRow {
    Message {
        message: Entity<MessageState>,
        options: MessageRenderOptions,
    },
    Summary {
        turn_id: EntityId,
        duration: Duration,
        expanded: bool,
    },
}

impl HistoryRow {
    fn message(message: Entity<MessageState>) -> Self {
        Self::Message {
            message,
            options: MessageRenderOptions {
                collapse_tools: true,
                ..Default::default()
            },
        }
    }
}

fn subscribe_to_chat(
    chat: Option<&Entity<ChatState>>,
    cx: &mut Context<ChatHistory>,
) -> Option<Subscription> {
    chat.map(|chat| {
        cx.observe(chat, |history, _, cx| {
            history.rebuild_transcript(cx);
            cx.notify();
        })
    })
}

pub(super) fn render_transcript_block(
    history: &WeakEntity<ChatHistory>,
    target: TranscriptBlockTarget,
    _window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    match target {
        TranscriptBlockTarget::AssistantHeader { label, .. } => {
            render_assistant_header(label, cx.theme()).into_any_element()
        }
        TranscriptBlockTarget::WorkedSummary {
            turn_id,
            duration,
            expanded,
        } => {
            let history = history.clone();
            render_worked_summary(duration, cx.theme(), expanded)
                .id(format!("worked-summary-{turn_id}"))
                .on_click(move |_, _, cx| {
                    let _ = history.update(cx, |history, cx| {
                        if !history.expanded_turns.remove(&turn_id) {
                            history.expanded_turns.insert(turn_id);
                        }
                        history.rebuild_transcript(cx);
                        cx.notify();
                    });
                })
                .into_any_element()
        }
        TranscriptBlockTarget::User { key } => {
            let Some((chat, message)) = transcript_message(history, &key, cx) else {
                return div().into_any_element();
            };
            let text = {
                let chat = chat.read(cx);
                let message = message.read(cx);
                let Some(ThreadItem::UserMessage { content, .. }) = chat.item_for_state(message)
                else {
                    return div().into_any_element();
                };
                user_input_text(content)
            };
            render_user_message(&text, cx.theme()).into_any_element()
        }
        TranscriptBlockTarget::Tools {
            key,
            collapse,
            active_tail,
        } => {
            let Some((chat, message)) = transcript_message(history, &key, cx) else {
                return div().into_any_element();
            };
            let message_for_toggle = message.clone();
            let chat = chat.read(cx);
            let message_state = message.read(cx);
            let tools = chat.tools_for_state(message_state);
            render_tool_group(
                &tools,
                collapse,
                active_tail,
                message_state.tools_expanded,
                cx.theme(),
                move |_, _, cx| {
                    message_for_toggle.update(cx, |message, cx| {
                        message.toggle_tools();
                        cx.notify();
                    });
                },
            )
            .into_any_element()
        }
    }
}

fn transcript_message(
    history: &WeakEntity<ChatHistory>,
    key: &HistoryKey,
    cx: &App,
) -> Option<(Entity<ChatState>, Entity<MessageState>)> {
    let chat = history.upgrade()?.read(cx).active_chat.clone()?;
    let message = chat
        .read(cx)
        .messages
        .iter()
        .find(|message| &message.read(cx).key == key)?
        .clone();
    Some((chat, message))
}

struct TurnFold {
    final_index: usize,
    duration: Duration,
}

fn completed_turn_fold(
    chat: &Entity<ChatState>,
    messages: &[Entity<MessageState>],
    user_index: usize,
    next_turn: usize,
    cx: &mut Context<ChatHistory>,
) -> Option<TurnFold> {
    let final_index = (user_index + 1..next_turn).rev().find(|index| {
        let message = messages[*index].read(cx);
        let chat = chat.read(cx);
        let Some(ThreadItem::AgentMessage { text, phase, .. }) = chat.item_for_state(message)
        else {
            return false;
        };
        !text.trim().is_empty()
            && matches!(message.stream_state, StreamState::Complete)
            && is_final_answer(phase.as_ref())
            && chat.has_done_tools_for_state(message)
    })?;

    if next_turn == messages.len()
        && has_working_message(chat, &messages[user_index + 1..next_turn], cx)
    {
        return None;
    }

    let has_progress = (user_index + 1..next_turn).any(|index| index != final_index);
    let final_has_tools = !chat
        .read(cx)
        .tools_for_state(messages[final_index].read(cx))
        .is_empty();
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

fn has_working_message(
    chat: &Entity<ChatState>,
    messages: &[Entity<MessageState>],
    cx: &mut Context<ChatHistory>,
) -> bool {
    messages.iter().any(|message| {
        let message = message.read(cx);
        if !matches!(message.stream_state, StreamState::Streaming) {
            return false;
        }
        match chat.read(cx).item_for_state(message) {
            Some(ThreadItem::AgentMessage { .. }) => true,
            Some(item) => is_tool_item(item),
            None => false,
        }
    })
}

fn is_active_tool_tail(
    chat: &Entity<ChatState>,
    messages: &[Entity<MessageState>],
    index: usize,
    cx: &mut Context<ChatHistory>,
) -> bool {
    if index + 1 != messages.len() {
        return false;
    }

    let message = messages[index].read(cx);
    matches!(message.stream_state, StreamState::Streaming)
        && !chat.read(cx).tools_for_state(message).is_empty()
}

fn is_user_message(
    chat: &Entity<ChatState>,
    message: &Entity<MessageState>,
    cx: &mut Context<ChatHistory>,
) -> bool {
    matches!(
        chat.read(cx).item_for_state(message.read(cx)),
        Some(ThreadItem::UserMessage { .. })
    )
}

fn next_user_index(
    chat: &Entity<ChatState>,
    messages: &[Entity<MessageState>],
    start: usize,
    cx: &mut Context<ChatHistory>,
) -> Option<usize> {
    (start..messages.len()).find(|index| is_user_message(chat, &messages[*index], cx))
}

fn is_final_answer(phase: Option<&MessagePhase>) -> bool {
    matches!(phase, Some(MessagePhase::FinalAnswer) | None)
}

fn is_tool_item(item: &ThreadItem) -> bool {
    matches!(
        item,
        ThreadItem::CommandExecution { .. }
            | ThreadItem::FileChange { .. }
            | ThreadItem::McpToolCall { .. }
            | ThreadItem::DynamicToolCall { .. }
    )
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
