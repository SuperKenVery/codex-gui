use super::{CodexGui, thread_mapping::*};
use crate::bridge::BridgeEvent;
use crate::gui::{
    ChatState, HistoryEntryKind, HistoryKey, MessageState, ProjectState, StreamState,
};
use codex_app_server_protocol::{ServerNotification, Thread, ThreadItem};
use gpui::{AppContext, Context, Entity};

impl CodexGui {
    pub(super) fn drain_bridge_events(&mut self, cx: &mut Context<Self>) {
        while let Ok(event) = self.bridge_rx.try_recv() {
            self.apply_bridge_event(event, cx);
        }
    }

    fn apply_bridge_event(&mut self, event: BridgeEvent, cx: &mut Context<Self>) {
        match event {
            BridgeEvent::Notification(notification) => {
                self.apply_server_notification(notification, cx)
            }
            BridgeEvent::RpcError(error) => self.apply_bridge_error(error.error.message, cx),
            BridgeEvent::TransportError(message) => self.apply_bridge_error(message, cx),
            BridgeEvent::Stderr(status) => self.set_bridge_status(status, cx),
        }
    }

    fn apply_server_notification(
        &mut self,
        notification: ServerNotification,
        cx: &mut Context<Self>,
    ) {
        match notification {
            ServerNotification::ThreadStarted(params) => {
                self.apply_thread_started(params.thread, cx);
            }
            ServerNotification::ThreadNameUpdated(params) => {
                if let Some(thread_name) = params.thread_name.filter(|name| !name.is_empty()) {
                    self.update_chat_title(&params.thread_id, thread_name, cx);
                }
            }
            ServerNotification::TurnStarted(params) => {
                self.upsert_thread_turn(&params.thread_id, params.turn.clone(), cx);
                self.ui_state.update(cx, |state, cx| {
                    state.start_turn(params.thread_id, params.turn.id);
                    cx.notify();
                });
                self.set_bridge_status("turn running", cx);
            }
            ServerNotification::ItemStarted(params) => {
                self.append_thread_item_data(&params.thread_id, params.item.clone(), cx);
                self.apply_item_started(&params.thread_id, params.item, cx);
            }
            ServerNotification::AgentMessageDelta(params) => {
                self.append_thread_agent_delta(
                    &params.thread_id,
                    &params.item_id,
                    &params.delta,
                    cx,
                );
                self.append_agent_delta(&params.thread_id, &params.item_id, &params.delta, cx);
            }
            ServerNotification::CommandExecutionOutputDelta(params) => {
                self.append_thread_command_output_delta(
                    &params.thread_id,
                    &params.item_id,
                    &params.delta,
                    cx,
                );
                self.touch_tool_message(&params.thread_id, &params.item_id, cx);
            }
            ServerNotification::FileChangeOutputDelta(params) => {
                self.append_thread_file_change_output_delta(
                    &params.thread_id,
                    &params.item_id,
                    &params.delta,
                    cx,
                );
                self.touch_tool_message(&params.thread_id, &params.item_id, cx);
            }
            ServerNotification::FileChangePatchUpdated(params) => {
                self.update_thread_file_change_item(
                    &params.thread_id,
                    &params.item_id,
                    params.changes.clone(),
                    cx,
                );
                self.touch_tool_message(&params.thread_id, &params.item_id, cx);
            }
            ServerNotification::ItemCompleted(params) => {
                self.replace_thread_item_data(&params.thread_id, params.item.clone(), cx);
                self.apply_item_completed(&params.thread_id, params.item, cx);
            }
            ServerNotification::ThreadStatusChanged(params) => {
                self.update_thread_status(&params.thread_id, params.status.clone(), cx);
                self.set_bridge_status(
                    format!("thread {}", thread_status_label(&params.status)),
                    cx,
                );
            }
            ServerNotification::TurnCompleted(params) => {
                let turn = params.turn;
                let thread_id = params.thread_id;
                let turn_id = turn.id.clone();
                self.upsert_thread_turn(&thread_id, turn, cx);
                self.ui_state.update(cx, |state, cx| {
                    state.finish_turn(&thread_id, &turn_id);
                    cx.notify();
                });
                self.finish_completed_tool_messages(&thread_id, cx);
                self.notify_chat(&thread_id, cx);
                self.set_bridge_status("turn complete", cx);
            }
            ServerNotification::Error(params) => {
                self.apply_bridge_error(params.error.message, cx);
            }
            _ => {}
        }
    }

    pub(super) fn apply_thread_started(&mut self, thread: Thread, cx: &mut Context<Self>) {
        let thread_id = thread.id.clone();
        let chat = chat_entity_from_thread(thread, cx);
        if let Some(project) = self.active_project_entity(cx) {
            project.update(cx, |project, cx| {
                project.upsert_chat(chat, &thread_id, cx);
                cx.notify();
            });
        }
        self.state.update(cx, |state, cx| {
            state.select_first_chat();
            cx.notify();
        });
        self.ui_state.update(cx, |state, cx| {
            state.close_new_chat();
            cx.notify();
        });
        self.set_bridge_status("thread ready", cx);
        if let Some(text) = self.pending_turn_text.take() {
            let settings = self.state.read(cx).chat_settings.clone();
            self.request_send_turn(thread_id, text, settings, cx);
            self.set_bridge_status("turn running", cx);
        }
    }

    pub(super) fn apply_thread_resumed(&mut self, thread: Thread, cx: &mut Context<Self>) {
        let thread_id = thread.id.clone();
        let chat = chat_entity_from_thread(thread, cx);
        if let Some(project) = self.active_project_entity(cx) {
            let should_keep_selected = self
                .active_chat_entity(cx)
                .map(|chat| chat.read(cx).id == thread_id)
                .unwrap_or(false);
            let loaded_chat_index = project.update(cx, |project, cx| {
                let loaded_chat_index = project.upsert_chat(chat, &thread_id, cx);
                cx.notify();
                loaded_chat_index
            });
            if should_keep_selected {
                self.state.update(cx, |state, cx| {
                    state.select_chat(loaded_chat_index);
                    cx.notify();
                });
            }
        }
        self.set_bridge_status("thread loaded", cx);
    }

    pub(super) fn apply_bridge_error(&mut self, message: String, cx: &mut Context<Self>) {
        self.ui_state.update(cx, |state, cx| {
            state.clear_active_turn();
            cx.notify();
        });
        self.set_bridge_status("codex app-server error", cx);
        if let Some(chat) = self.active_chat_entity(cx) {
            let thread_id = chat.read(cx).id.clone();
            self.append_notice(&thread_id, message, cx);
        } else if let Some(project) = self.active_project_entity(cx) {
            let chat = cx.new(|cx| {
                ChatState::new(
                    "bridge-error".into(),
                    "Bridge error".into(),
                    message.clone().into(),
                    vec![cx.new(|cx| MessageState::notice(message, cx))],
                )
            });
            project.update(cx, |project, cx| {
                project.append_chat(chat);
                cx.notify();
            });
        }
    }

    fn apply_item_started(&self, thread_id: &str, item: ThreadItem, cx: &mut Context<Self>) {
        match item {
            ThreadItem::UserMessage { id, content, .. } => {
                let text = user_input_text(&content);
                if !text.is_empty() {
                    self.append_message_state(
                        thread_id,
                        HistoryKey::Item(id),
                        HistoryEntryKind::User,
                        None,
                        StreamState::Complete,
                        cx,
                    );
                }
            }
            ThreadItem::AgentMessage { id, text, .. } => {
                self.finish_completed_tool_messages(thread_id, cx);
                self.append_message_state(
                    thread_id,
                    HistoryKey::Item(id),
                    HistoryEntryKind::Assistant,
                    Some(text),
                    StreamState::Streaming,
                    cx,
                );
            }
            ThreadItem::CommandExecution { id, .. } => {
                self.append_or_update_tool(thread_id, &id, cx);
            }
            ThreadItem::FileChange { id, .. } => {
                self.append_or_update_tool(thread_id, &id, cx);
            }
            ThreadItem::McpToolCall { id, .. } => {
                self.append_or_update_tool(thread_id, &id, cx);
            }
            ThreadItem::DynamicToolCall { id, .. } => {
                self.append_or_update_tool(thread_id, &id, cx);
            }
            _ => {}
        }
    }

    fn apply_item_completed(&self, thread_id: &str, item: ThreadItem, cx: &mut Context<Self>) {
        match item {
            ThreadItem::CommandExecution { id, .. } => {
                self.append_or_update_tool(thread_id, &id, cx);
                self.mark_item_complete(thread_id, &id, cx);
            }
            ThreadItem::FileChange { id, .. } => {
                self.append_or_update_tool(thread_id, &id, cx);
                self.mark_item_complete(thread_id, &id, cx);
            }
            ThreadItem::McpToolCall { id, .. } => {
                self.append_or_update_tool(thread_id, &id, cx);
                self.mark_item_complete(thread_id, &id, cx);
            }
            ThreadItem::DynamicToolCall { id, .. } => {
                self.append_or_update_tool(thread_id, &id, cx);
                self.mark_item_complete(thread_id, &id, cx);
            }
            item => self.mark_item_complete(thread_id, item.id(), cx),
        }
        self.notify_chat(thread_id, cx);
    }

    pub(super) fn active_project_entity(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<Entity<ProjectState>> {
        self.state.read(cx).active_project()
    }

    pub(super) fn active_chat_entity(&self, cx: &mut Context<Self>) -> Option<Entity<ChatState>> {
        let (project, active_chat) = {
            let state = self.state.read(cx);
            (state.active_project(), state.active_chat)
        };
        project.and_then(|project| {
            let chats = project.read(cx).chats.clone();
            chats.get(active_chat).cloned()
        })
    }

    fn find_chat_entity(
        &self,
        thread_id: &str,
        cx: &mut Context<Self>,
    ) -> Option<Entity<ChatState>> {
        let project = self.active_project_entity(cx)?;
        let chats = project.read(cx).chats.clone();
        for chat in chats {
            let is_match = chat.read(cx).id == thread_id;
            if is_match {
                return Some(chat);
            }
        }
        None
    }

    fn notify_chat(&self, thread_id: &str, cx: &mut Context<Self>) {
        let Some(chat) = self.find_chat_entity(thread_id, cx) else {
            return;
        };
        chat.update(cx, |_, cx| cx.notify());
    }

    fn append_notice(&self, thread_id: &str, body: String, cx: &mut Context<Self>) {
        let key = HistoryKey::Notice(format!("notice-{thread_id}"));
        self.append_message_state(
            thread_id,
            key,
            HistoryEntryKind::Notice,
            Some(body),
            StreamState::Complete,
            cx,
        );
    }

    fn append_message_state(
        &self,
        thread_id: &str,
        key: HistoryKey,
        kind: HistoryEntryKind,
        body: Option<String>,
        stream_state: StreamState,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.find_chat_entity(thread_id, cx) else {
            return;
        };
        if let Some(existing) = self.find_message_by_key(thread_id, &key, cx) {
            existing.update(cx, |message, cx| {
                message.stream_state = stream_state;
                if let Some(body) = body {
                    message.rendered_body = body;
                    message.sync_body_view(cx);
                }
                message.touch();
                cx.notify();
            });
            return;
        }
        let message = cx.new(|cx| MessageState::item(key, kind, body, stream_state, cx));
        chat.update(cx, |chat, cx| {
            chat.append_message(message);
            cx.notify();
        });
    }

    fn update_chat_title(&self, thread_id: &str, title: String, cx: &mut Context<Self>) {
        let Some(chat) = self.find_chat_entity(thread_id, cx) else {
            return;
        };
        chat.update(cx, |chat, cx| {
            chat.set_title(title);
            cx.notify();
        });
    }

    fn upsert_thread_turn(
        &self,
        thread_id: &str,
        turn: codex_app_server_protocol::Turn,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.find_chat_entity(thread_id, cx) else {
            return;
        };
        chat.update(cx, |chat, cx| {
            chat.upsert_turn(turn);
            cx.notify();
        });
    }

    fn append_thread_item_data(&self, thread_id: &str, item: ThreadItem, cx: &mut Context<Self>) {
        let active_turn_id = self
            .ui_state
            .read(cx)
            .active_turn
            .as_ref()
            .filter(|active_turn| active_turn.thread_id == thread_id)
            .map(|active_turn| active_turn.turn_id.clone());
        let Some(chat) = self.find_chat_entity(thread_id, cx) else {
            return;
        };
        chat.update(cx, |chat, cx| {
            chat.append_thread_item(active_turn_id.as_deref(), item);
            cx.notify();
        });
    }

    fn replace_thread_item_data(&self, thread_id: &str, item: ThreadItem, cx: &mut Context<Self>) {
        let Some(chat) = self.find_chat_entity(thread_id, cx) else {
            return;
        };
        chat.update(cx, |chat, cx| {
            chat.replace_thread_item(item);
            cx.notify();
        });
    }

    fn append_thread_agent_delta(
        &self,
        thread_id: &str,
        item_id: &str,
        delta: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.find_chat_entity(thread_id, cx) else {
            return;
        };
        chat.update(cx, |chat, _| {
            chat.append_agent_text_delta(item_id, delta);
        });
    }

    fn append_thread_command_output_delta(
        &self,
        thread_id: &str,
        item_id: &str,
        delta: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.find_chat_entity(thread_id, cx) else {
            return;
        };
        chat.update(cx, |chat, _| {
            chat.append_command_output_delta(item_id, delta);
        });
    }

    fn append_thread_file_change_output_delta(
        &self,
        thread_id: &str,
        item_id: &str,
        delta: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.find_chat_entity(thread_id, cx) else {
            return;
        };
        chat.update(cx, |chat, _| {
            chat.append_file_change_output_delta(item_id, delta);
        });
    }

    fn update_thread_file_change_item(
        &self,
        thread_id: &str,
        item_id: &str,
        changes: Vec<codex_app_server_protocol::FileUpdateChange>,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.find_chat_entity(thread_id, cx) else {
            return;
        };
        chat.update(cx, |chat, _| {
            chat.update_file_change_item(item_id, changes);
        });
    }

    fn update_thread_status(
        &self,
        thread_id: &str,
        status: codex_app_server_protocol::ThreadStatus,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.find_chat_entity(thread_id, cx) else {
            return;
        };
        chat.update(cx, |chat, cx| {
            chat.set_thread_status(status);
            cx.notify();
        });
    }

    fn append_agent_delta(
        &self,
        thread_id: &str,
        item_id: &str,
        delta: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(message) = self.find_assistant_message(thread_id, item_id, cx) else {
            self.append_message_state(
                thread_id,
                HistoryKey::Item(item_id.to_string()),
                HistoryEntryKind::Assistant,
                Some(delta.to_string()),
                StreamState::Streaming,
                cx,
            );
            return;
        };
        message.update(cx, |message, cx| {
            message.mark_streaming();
            message.append_body_view_delta(delta, cx);
            cx.notify();
        });
    }

    fn append_or_update_tool(&self, thread_id: &str, tool_id: &str, cx: &mut Context<Self>) {
        if let Some(message) = self.find_latest_streaming_assistant_message(thread_id, cx) {
            message.update(cx, |message, cx| {
                message.touch();
                cx.notify();
            });
        } else if let Some(message) = self.find_tool_message(thread_id, tool_id, cx) {
            message.update(cx, |message, cx| {
                message.mark_streaming();
                cx.notify();
            });
        } else {
            self.append_message_state(
                thread_id,
                HistoryKey::ToolGroup(tool_id.to_string()),
                HistoryEntryKind::ToolGroup,
                None,
                StreamState::Streaming,
                cx,
            );
        }
    }

    fn finish_completed_tool_messages(&self, thread_id: &str, cx: &mut Context<Self>) {
        let Some(chat) = self.find_chat_entity(thread_id, cx) else {
            return;
        };
        let messages = chat.read(cx).messages.clone();
        for message in messages {
            message.update(cx, |message, cx| {
                if !matches!(message.stream_state, StreamState::Streaming)
                    || !message.rendered_body.is_empty()
                    || !chat.read(cx).has_done_tools_for_state(message)
                {
                    return;
                }
                message.mark_complete(cx);
                cx.notify();
            });
        }
    }

    fn touch_tool_message(&self, thread_id: &str, item_id: &str, cx: &mut Context<Self>) {
        let Some(message) = self.find_tool_message(thread_id, item_id, cx) else {
            return;
        };
        message.update(cx, |message, cx| {
            message.touch();
            cx.notify();
        });
    }

    fn mark_item_complete(&self, thread_id: &str, item_id: &str, cx: &mut Context<Self>) {
        if let Some(message) = self.find_assistant_message(thread_id, item_id, cx) {
            message.update(cx, |message, cx| {
                message.mark_complete(cx);
                cx.notify();
            });
            return;
        }

        if let Some(message) = self.find_tool_message(thread_id, item_id, cx) {
            let tools_done = self
                .find_chat_entity(thread_id, cx)
                .map(|chat| chat.read(cx).has_done_tools_for_state(&message.read(cx)))
                .unwrap_or(false);
            message.update(cx, |message, cx| {
                if tools_done {
                    message.mark_complete(cx);
                } else {
                    message.touch();
                }
                cx.notify();
            });
        }
    }

    fn find_message_by_key(
        &self,
        thread_id: &str,
        key: &HistoryKey,
        cx: &mut Context<Self>,
    ) -> Option<Entity<MessageState>> {
        let chat = self.find_chat_entity(thread_id, cx)?;
        let messages = chat.read(cx).messages.clone();
        messages
            .into_iter()
            .rev()
            .find(|message| &message.read(cx).key == key)
    }

    fn find_assistant_message(
        &self,
        thread_id: &str,
        item_id: &str,
        cx: &mut Context<Self>,
    ) -> Option<Entity<MessageState>> {
        let chat = self.find_chat_entity(thread_id, cx)?;
        let messages = chat.read(cx).messages.clone();
        messages.into_iter().rev().find(|message| {
            message.read(cx).key == HistoryKey::Item(item_id.to_string())
                && chat.read(cx).item_is_agent_message(item_id)
        })
    }

    fn find_latest_streaming_assistant_message(
        &self,
        thread_id: &str,
        cx: &mut Context<Self>,
    ) -> Option<Entity<MessageState>> {
        let chat = self.find_chat_entity(thread_id, cx)?;
        let messages = chat.read(cx).messages.clone();
        messages.into_iter().rev().find(|message| {
            let message = message.read(cx);
            matches!(message.stream_state, StreamState::Streaming)
                && matches!(
                    &message.key,
                    HistoryKey::Item(item_id) if chat.read(cx).item_is_agent_message(item_id)
                )
        })
    }

    fn find_tool_message(
        &self,
        thread_id: &str,
        item_id: &str,
        cx: &mut Context<Self>,
    ) -> Option<Entity<MessageState>> {
        let chat = self.find_chat_entity(thread_id, cx)?;
        let messages = chat.read(cx).messages.clone();
        messages
            .into_iter()
            .rev()
            .find(|message| chat.read(cx).message_has_tool(&message.read(cx), item_id))
    }
}
