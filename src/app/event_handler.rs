use super::{CodexGui, thread_mapping::*};
use crate::bridge::BridgeEvent;
use crate::gui::{
    ActiveTurn, AssistantPhase, ChatState, FileChangeSummary, Message, MessageState, ProjectState,
    StreamState, ToolCall, ToolStatus,
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
                self.ui_state.update(cx, |state, cx| {
                    state.active_turn = Some(ActiveTurn {
                        thread_id: params.thread_id,
                        turn_id: params.turn.id,
                    });
                    cx.notify();
                });
                self.set_bridge_status("turn running", cx);
            }
            ServerNotification::ItemStarted(params) => {
                self.apply_item_started(&params.thread_id, params.item, cx);
            }
            ServerNotification::AgentMessageDelta(params) => {
                self.append_agent_delta(&params.thread_id, &params.item_id, &params.delta, cx);
            }
            ServerNotification::CommandExecutionOutputDelta(params) => {
                self.append_tool_output_delta(
                    &params.thread_id,
                    &params.item_id,
                    &params.delta,
                    cx,
                );
            }
            ServerNotification::FileChangeOutputDelta(params) => {
                self.append_tool_output_delta(
                    &params.thread_id,
                    &params.item_id,
                    &params.delta,
                    cx,
                );
            }
            ServerNotification::FileChangePatchUpdated(params) => {
                self.update_file_change_tool(
                    &params.thread_id,
                    &params.item_id,
                    summarize_file_changes(params.changes),
                    cx,
                );
            }
            ServerNotification::ItemCompleted(params) => {
                self.apply_item_completed(&params.thread_id, params.item, cx);
            }
            ServerNotification::ThreadStatusChanged(params) => {
                self.set_bridge_status(
                    format!("thread {}", thread_status_label(&params.status)),
                    cx,
                );
            }
            ServerNotification::TurnCompleted(params) => {
                let thread_id = params.thread_id;
                let turn_id = params.turn.id;
                self.ui_state.update(cx, |state, cx| {
                    if state.active_turn.as_ref().is_some_and(|active_turn| {
                        active_turn.thread_id == thread_id && active_turn.turn_id == turn_id
                    }) {
                        state.active_turn = None;
                        cx.notify();
                    }
                });
                self.finish_completed_tool_messages(&thread_id, cx);
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
                upsert_chat(project, chat, &thread_id, cx);
                cx.notify();
            });
        }
        self.state.update(cx, |state, cx| {
            state.active_chat = 0;
            cx.notify();
        });
        self.ui_state.update(cx, |state, cx| {
            state.new_chat_open = false;
            cx.notify();
        });
        self.set_bridge_status("thread ready", cx);
        if let Some(text) = self.pending_turn_text.take() {
            let settings = self.state.read(cx).chat_settings.clone();
            self.send_turn_request(thread_id, text, settings, cx);
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
                upsert_chat(project, chat, &thread_id, cx);
                let loaded_chat_index = project
                    .chats
                    .iter()
                    .position(|chat| chat.read(cx).id == thread_id)
                    .unwrap_or(0);
                cx.notify();
                loaded_chat_index
            });
            if should_keep_selected {
                self.state.update(cx, |state, cx| {
                    state.active_chat = loaded_chat_index;
                    cx.notify();
                });
            }
        }
        self.set_bridge_status("thread loaded", cx);
    }

    pub(super) fn apply_bridge_error(&mut self, message: String, cx: &mut Context<Self>) {
        self.ui_state.update(cx, |state, cx| {
            state.active_turn = None;
            cx.notify();
        });
        self.set_bridge_status("codex app-server error", cx);
        if let Some(chat) = self.active_chat_entity(cx) {
            let thread_id = chat.read(cx).id.clone();
            self.append_message(&thread_id, Message::Notice(message), cx);
        } else if let Some(project) = self.active_project_entity(cx) {
            let chat = cx.new(|cx| {
                ChatState::new(
                    "bridge-error".into(),
                    "Bridge error".into(),
                    message.clone().into(),
                    vec![cx.new(|cx| MessageState::new(Message::Notice(message), cx))],
                )
            });
            project.update(cx, |project, cx| {
                project.chats.push(chat);
                cx.notify();
            });
        }
    }

    fn apply_item_started(&self, thread_id: &str, item: ThreadItem, cx: &mut Context<Self>) {
        match item {
            ThreadItem::UserMessage { content, .. } => {
                let text = user_input_text(&content);
                if !text.is_empty() {
                    self.append_message(thread_id, Message::User(text), cx);
                }
            }
            ThreadItem::AgentMessage {
                id, text, phase, ..
            } => {
                self.finish_completed_tool_messages(thread_id, cx);
                self.append_message(
                    thread_id,
                    Message::Assistant {
                        id,
                        body: text,
                        state: StreamState::Streaming,
                        phase: assistant_phase(phase.as_ref()),
                        tools: Vec::new(),
                    },
                    cx,
                );
            }
            ThreadItem::CommandExecution {
                id, command, cwd, ..
            } => {
                self.append_or_update_tool(
                    thread_id,
                    ToolCall::Command {
                        id,
                        command,
                        cwd: cwd.display().to_string(),
                        status: ToolStatus::Running,
                    },
                    cx,
                );
            }
            ThreadItem::FileChange {
                id,
                changes,
                status,
            } => {
                self.append_or_update_tool(
                    thread_id,
                    ToolCall::FileChange {
                        id,
                        changes: summarize_file_changes(changes),
                        status: tool_status_from_patch(&status),
                    },
                    cx,
                );
            }
            ThreadItem::McpToolCall {
                id, server, tool, ..
            } => {
                self.append_or_update_tool(
                    thread_id,
                    ToolCall::Mcp {
                        id,
                        server,
                        tool,
                        status: ToolStatus::Running,
                    },
                    cx,
                );
            }
            ThreadItem::DynamicToolCall {
                id,
                namespace,
                tool,
                ..
            } => {
                self.append_or_update_tool(
                    thread_id,
                    ToolCall::Dynamic {
                        id,
                        namespace,
                        tool,
                        status: ToolStatus::Running,
                    },
                    cx,
                );
            }
            _ => {}
        }
    }

    fn apply_item_completed(&self, thread_id: &str, item: ThreadItem, cx: &mut Context<Self>) {
        match item {
            ThreadItem::CommandExecution {
                id,
                command,
                cwd,
                status,
                ..
            } => {
                self.append_or_update_tool(
                    thread_id,
                    ToolCall::Command {
                        id: id.clone(),
                        command,
                        cwd: cwd.display().to_string(),
                        status: tool_status_from_command(&status),
                    },
                    cx,
                );
                self.mark_item_complete(thread_id, &id, cx);
            }
            ThreadItem::FileChange {
                id,
                changes,
                status,
            } => {
                self.append_or_update_tool(
                    thread_id,
                    ToolCall::FileChange {
                        id: id.clone(),
                        changes: summarize_file_changes(changes),
                        status: tool_status_from_patch(&status),
                    },
                    cx,
                );
                self.mark_item_complete(thread_id, &id, cx);
            }
            ThreadItem::McpToolCall {
                id,
                server,
                tool,
                status,
                ..
            } => {
                self.append_or_update_tool(
                    thread_id,
                    ToolCall::Mcp {
                        id: id.clone(),
                        server,
                        tool,
                        status: tool_status_from_mcp(&status),
                    },
                    cx,
                );
                self.mark_item_complete(thread_id, &id, cx);
            }
            ThreadItem::DynamicToolCall {
                id,
                namespace,
                tool,
                status,
                ..
            } => {
                self.append_or_update_tool(
                    thread_id,
                    ToolCall::Dynamic {
                        id: id.clone(),
                        namespace,
                        tool,
                        status: tool_status_from_dynamic(&status),
                    },
                    cx,
                );
                self.mark_item_complete(thread_id, &id, cx);
            }
            item => self.mark_item_complete(thread_id, item.id(), cx),
        }
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

    fn append_message(&self, thread_id: &str, message: Message, cx: &mut Context<Self>) {
        let Some(chat) = self.find_chat_entity(thread_id, cx) else {
            return;
        };
        let message = cx.new(|cx| MessageState::new(message, cx));
        chat.update(cx, |chat, cx| {
            chat.messages.push(message);
            cx.notify();
        });
    }

    fn update_chat_title(&self, thread_id: &str, title: String, cx: &mut Context<Self>) {
        let Some(chat) = self.find_chat_entity(thread_id, cx) else {
            return;
        };
        chat.update(cx, |chat, cx| {
            chat.title = title.into();
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
            self.append_message(
                thread_id,
                Message::Assistant {
                    id: item_id.to_string(),
                    body: delta.to_string(),
                    state: StreamState::Streaming,
                    phase: AssistantPhase::FinalAnswer,
                    tools: Vec::new(),
                },
                cx,
            );
            return;
        };
        message.update(cx, |message, cx| {
            if let Message::Assistant { body, state, .. } = &mut message.message {
                body.push_str(delta);
                *state = StreamState::Streaming;
                message.touch();
                message.append_body_view_delta(delta, cx);
                cx.notify();
            }
        });
    }

    fn append_or_update_tool(&self, thread_id: &str, tool: ToolCall, cx: &mut Context<Self>) {
        if let Some(message) = self.find_latest_streaming_assistant_message(thread_id, cx) {
            message.update(cx, |message, cx| {
                if let Message::Assistant { tools, .. } = &mut message.message {
                    if let Some(existing) =
                        tools.iter_mut().find(|existing| existing.id() == tool.id())
                    {
                        *existing = tool;
                    } else {
                        tools.push(tool);
                    }
                    message.touch();
                    cx.notify();
                }
            });
        } else {
            self.append_message(
                thread_id,
                Message::Assistant {
                    id: format!("tool-group-{}", tool.id()),
                    body: String::new(),
                    state: StreamState::Streaming,
                    phase: AssistantPhase::Commentary,
                    tools: vec![tool],
                },
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
                let Message::Assistant {
                    state, tools, body, ..
                } = &mut message.message
                else {
                    return;
                };
                if !matches!(*state, StreamState::Streaming) || tools.is_empty() {
                    return;
                }
                if tools
                    .iter()
                    .all(|tool| matches!(tool.status(), ToolStatus::Done))
                    && body.is_empty()
                {
                    *state = StreamState::Complete;
                    message.touch();
                    message.sync_body_view(cx);
                    cx.notify();
                }
            });
        }
    }

    fn append_tool_output_delta(
        &self,
        thread_id: &str,
        item_id: &str,
        delta: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(message) = self.find_tool_message(thread_id, item_id, cx) else {
            return;
        };
        message.update(cx, |message, cx| {
            if let Message::Assistant { tools, .. } = &mut message.message {
                if let Some(ToolCall::FileChange { changes, .. }) =
                    tools.iter_mut().find(|tool| tool.id() == item_id)
                {
                    if let Some(last_change) = changes.last_mut() {
                        let stats = diff_stats(delta);
                        last_change.additions += stats.additions;
                        last_change.deletions += stats.deletions;
                    }
                    message.touch();
                    cx.notify();
                }
            }
        });
    }

    fn update_file_change_tool(
        &self,
        thread_id: &str,
        item_id: &str,
        changes: Vec<FileChangeSummary>,
        cx: &mut Context<Self>,
    ) {
        let Some(message) = self.find_tool_message(thread_id, item_id, cx) else {
            return;
        };
        message.update(cx, |message, cx| {
            if let Message::Assistant { tools, .. } = &mut message.message {
                if let Some(ToolCall::FileChange {
                    changes: existing, ..
                }) = tools.iter_mut().find(|tool| tool.id() == item_id)
                {
                    *existing = changes;
                    message.touch();
                    cx.notify();
                }
            }
        });
    }

    fn mark_item_complete(&self, thread_id: &str, item_id: &str, cx: &mut Context<Self>) {
        if let Some(message) = self.find_assistant_message(thread_id, item_id, cx) {
            message.update(cx, |message, cx| {
                if let Message::Assistant { state, .. } = &mut message.message {
                    *state = StreamState::Complete;
                    message.touch();
                    message.sync_body_view(cx);
                    cx.notify();
                }
            });
            return;
        }

        if let Some(message) = self.find_tool_message(thread_id, item_id, cx) {
            message.update(cx, |message, cx| {
                if let Message::Assistant { tools, .. } = &mut message.message {
                    if let Some(tool) = tools.iter_mut().find(|tool| tool.id() == item_id) {
                        tool.set_status(ToolStatus::Done);
                        message.touch();
                        cx.notify();
                    }
                }
            });
        }
    }

    fn find_assistant_message(
        &self,
        thread_id: &str,
        item_id: &str,
        cx: &mut Context<Self>,
    ) -> Option<Entity<MessageState>> {
        let chat = self.find_chat_entity(thread_id, cx)?;
        let messages = chat.read(cx).messages.clone();
        for message in messages.into_iter().rev() {
            let is_match = matches!(
                &message.read(cx).message,
                Message::Assistant { id, .. } if id == item_id
            );
            if is_match {
                return Some(message);
            }
        }
        None
    }

    fn find_latest_streaming_assistant_message(
        &self,
        thread_id: &str,
        cx: &mut Context<Self>,
    ) -> Option<Entity<MessageState>> {
        let chat = self.find_chat_entity(thread_id, cx)?;
        let messages = chat.read(cx).messages.clone();
        for message in messages.into_iter().rev() {
            let is_match = matches!(
                &message.read(cx).message,
                Message::Assistant {
                    state: StreamState::Streaming,
                    ..
                }
            );
            if is_match {
                return Some(message);
            }
        }
        None
    }

    fn find_tool_message(
        &self,
        thread_id: &str,
        item_id: &str,
        cx: &mut Context<Self>,
    ) -> Option<Entity<MessageState>> {
        let chat = self.find_chat_entity(thread_id, cx)?;
        let messages = chat.read(cx).messages.clone();
        for message in messages.into_iter().rev() {
            let is_match = matches!(
                &message.read(cx).message,
                Message::Assistant { tools, .. } if tools.iter().any(|tool| tool.id() == item_id)
            );
            if is_match {
                return Some(message);
            }
        }
        None
    }
}
