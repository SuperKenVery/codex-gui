use crate::bridge::{BridgeCommand, BridgeEvent, start_app_server_bridge};
use crate::gui::{
    ApprovalReviewerMode, BridgeState, ChatPanel, ChatState, GuiState, Message, MessageState,
    ProjectState, SideChat, Sidebar, StreamState, ToolCall, ToolStatus, UiState,
};
use crate::workspace::workspace_path;
use codex_app_server_protocol::{
    CommandExecutionStatus, DynamicToolCallStatus, McpToolCallStatus, Thread, ThreadItem,
    ThreadStatus, UserInput,
};
use gpui::{
    Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Task, Window, div,
    prelude::*, transparent_black,
};
use gpui_component::ActiveTheme as _;
use std::{
    path::Path,
    sync::mpsc::{Receiver, Sender},
    time::Duration,
};

pub struct CodexGui {
    state: Entity<GuiState>,
    ui_state: Entity<UiState>,
    bridge_state: Entity<BridgeState>,
    bridge_tx: Option<Sender<BridgeCommand>>,
    bridge_rx: Receiver<BridgeEvent>,
    pending_turn_text: Option<String>,
    sidebar: Entity<Sidebar>,
    chat_panel: Entity<ChatPanel>,
    side_chat: Entity<SideChat>,
    _bridge_task: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl CodexGui {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (bridge_tx, bridge_rx) = start_app_server_bridge();
        let initial_project =
            cx.new(|_| ProjectState::new("codex-gui".into(), workspace_path().into(), Vec::new()));
        let state = cx.new(|_| GuiState::new(vec![initial_project]));
        let ui_state = cx.new(|_| UiState::new());
        let bridge_state = cx.new(|_| BridgeState::new());
        let parent = cx.entity().downgrade();
        let sidebar = cx.new(|cx| Sidebar::new(parent.clone(), state.clone(), cx));
        let chat_panel = cx.new(|cx| {
            ChatPanel::new(
                parent.clone(),
                state.clone(),
                ui_state.clone(),
                bridge_state.clone(),
                window,
                cx,
            )
        });
        let side_chat = cx.new(|cx| SideChat::new(state.clone(), cx));

        let bridge_task = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                if this
                    .update(cx, |view, cx| view.drain_bridge_events(cx))
                    .is_err()
                {
                    break;
                }
            }
        });

        Self {
            state,
            ui_state,
            bridge_state,
            bridge_tx: Some(bridge_tx),
            bridge_rx,
            pending_turn_text: None,
            sidebar,
            chat_panel,
            side_chat,
            _bridge_task: bridge_task,
            _subscriptions: Vec::new(),
        }
    }

    pub(crate) fn select_project(&mut self, index: usize, cx: &mut Context<Self>) {
        let cwd = self
            .state
            .read(cx)
            .projects
            .get(index)
            .map(|project| project.read(cx).path.to_string());
        let should_load_threads = self
            .state
            .read(cx)
            .projects
            .get(index)
            .map(|project| !project.read(cx).threads_loaded)
            .unwrap_or(false);
        self.state.update(cx, |state, cx| {
            state.active_project = index;
            state.active_chat = 0;
            cx.notify();
        });
        if should_load_threads {
            if let Some(cwd) = cwd {
                self.send_bridge(BridgeCommand::ListThreads { cwd }, cx);
                self.set_bridge_status("loading project threads", cx);
            }
        }
    }

    pub(crate) fn open_new_chat(&mut self, cx: &mut Context<Self>) {
        self.ui_state.update(cx, |state, cx| {
            state.new_chat_open = true;
            cx.notify();
        });
        cx.notify();
    }

    pub(crate) fn add_project(&mut self, path: String, cx: &mut Context<Self>) {
        let path = path.trim();
        if path.is_empty() {
            return;
        }

        let existing_index = self
            .state
            .read(cx)
            .projects
            .iter()
            .position(|project| project.read(cx).path.as_ref() == path);
        if let Some(index) = existing_index {
            self.select_project(index, cx);
            self.open_new_chat(cx);
            return;
        }

        let name = project_name_from_path(path);
        let project = cx.new(|_| ProjectState::new(name.into(), path.into(), Vec::new()));
        let index = self.state.update(cx, |state, cx| {
            state.projects.push(project);
            state.active_project = state.projects.len() - 1;
            state.active_chat = 0;
            cx.notify();
            state.active_project
        });
        self.ui_state.update(cx, |state, cx| {
            state.new_chat_open = true;
            cx.notify();
        });
        self.select_project(index, cx);
    }

    pub(crate) fn select_chat(&mut self, index: usize, cx: &mut Context<Self>) {
        let thread_id = self.state.read(cx).active_project().and_then(|project| {
            project
                .read(cx)
                .chats
                .get(index)
                .map(|chat| chat.read(cx).id.clone())
        });
        self.state.update(cx, |state, cx| {
            state.active_chat = index;
            cx.notify();
        });
        self.ui_state.update(cx, |state, cx| {
            state.new_chat_open = false;
            cx.notify();
        });
        if let Some(thread_id) = thread_id.filter(|thread_id| thread_id != "empty") {
            self.send_bridge(BridgeCommand::ResumeThread { thread_id }, cx);
            self.set_bridge_status("loading thread", cx);
        }
    }

    pub(crate) fn fork_chat(&mut self, cx: &mut Context<Self>) {
        let Some(thread_id) = self
            .active_chat_entity(cx)
            .map(|chat| chat.read(cx).id.clone())
        else {
            return;
        };
        self.send_bridge(BridgeCommand::ForkThread { thread_id }, cx);
        self.set_bridge_status("forking thread", cx);
    }

    pub(crate) fn start_thread(&mut self, cx: &mut Context<Self>) {
        let settings = self.state.read(cx).chat_settings.clone();
        let cwd = self
            .active_project_entity(cx)
            .map(|project| project.read(cx).path.to_string())
            .unwrap_or_else(workspace_path);
        self.send_bridge(BridgeCommand::StartThread { cwd, settings }, cx);
        self.set_bridge_status("starting thread", cx);
    }

    pub(crate) fn send_turn_text(&mut self, text: String, cx: &mut Context<Self>) {
        let Some(thread_id) = self
            .active_chat_entity(cx)
            .map(|chat| chat.read(cx).id.clone())
        else {
            self.pending_turn_text = Some(text);
            self.start_thread(cx);
            return;
        };
        if thread_id == "empty" {
            self.pending_turn_text = Some(text);
            self.start_thread(cx);
            return;
        }
        let settings = self.state.read(cx).chat_settings.clone();
        self.send_bridge(
            BridgeCommand::SendTurn {
                thread_id,
                text,
                settings,
            },
            cx,
        );
        self.set_bridge_status("turn running", cx);
    }

    pub(crate) fn set_model(&mut self, model: String, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| {
            state.chat_settings.model = model.clone();
            if let Some(option) = state
                .available_models
                .iter()
                .find(|option| option.id == state.chat_settings.model)
            {
                state.chat_settings.effort = option.default_effort.clone();
            }
            cx.notify();
        });
        self.update_active_thread_settings(cx);
    }

    pub(crate) fn set_effort(&mut self, effort: String, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| {
            state.chat_settings.effort = effort;
            cx.notify();
        });
        self.update_active_thread_settings(cx);
    }

    pub(crate) fn set_permission_profile(
        &mut self,
        permission_profile: String,
        cx: &mut Context<Self>,
    ) {
        self.state.update(cx, |state, cx| {
            state.chat_settings.permission_profile = permission_profile;
            cx.notify();
        });
        self.update_active_thread_settings(cx);
    }

    pub(crate) fn set_approvals_reviewer(
        &mut self,
        approvals_reviewer: ApprovalReviewerMode,
        cx: &mut Context<Self>,
    ) {
        self.state.update(cx, |state, cx| {
            state.chat_settings.approvals_reviewer = approvals_reviewer;
            cx.notify();
        });
        self.update_active_thread_settings(cx);
    }

    pub(crate) fn toggle_side_chat(&mut self, cx: &mut Context<Self>) {
        self.ui_state.update(cx, |state, cx| {
            state.side_chat_open = !state.side_chat_open;
            cx.notify();
        });
        cx.notify();
    }

    fn send_bridge(&mut self, command: BridgeCommand, cx: &mut Context<Self>) {
        if let Some(tx) = &self.bridge_tx {
            if tx.send(command).is_err() {
                self.set_bridge_status("codex app-server writer stopped", cx);
            }
        }
    }

    fn update_active_thread_settings(&mut self, cx: &mut Context<Self>) {
        let Some(thread_id) = self
            .active_chat_entity(cx)
            .map(|chat| chat.read(cx).id.clone())
            .filter(|thread_id| thread_id != "empty" && thread_id != "bridge-error")
        else {
            return;
        };
        let settings = self.state.read(cx).chat_settings.clone();
        self.send_bridge(
            BridgeCommand::UpdateThreadSettings {
                thread_id,
                settings,
            },
            cx,
        );
        self.set_bridge_status("updating settings", cx);
    }

    fn set_bridge_status(&self, status: impl Into<String>, cx: &mut Context<Self>) {
        self.bridge_state.update(cx, |state, cx| {
            state.status = status.into();
            cx.notify();
        });
    }

    fn drain_bridge_events(&mut self, cx: &mut Context<Self>) {
        while let Ok(event) = self.bridge_rx.try_recv() {
            self.apply_bridge_event(event, cx);
        }
    }

    fn apply_bridge_event(&mut self, event: BridgeEvent, cx: &mut Context<Self>) {
        match event {
            BridgeEvent::Status(status) => self.set_bridge_status(status, cx),
            BridgeEvent::ThreadsLoaded { cwd, threads } => {
                let chats = if threads.is_empty() {
                    vec![empty_chat_entity(cx)]
                } else {
                    threads
                        .into_iter()
                        .map(|thread| chat_entity_from_thread(thread, cx))
                        .collect::<Vec<_>>()
                };
                let default_thread_id = chats
                    .first()
                    .map(|chat| chat.read(cx).id.clone())
                    .filter(|thread_id| thread_id != "empty");
                let project_index = self
                    .state
                    .read(cx)
                    .projects
                    .iter()
                    .position(|project| project.read(cx).path.as_ref() == cwd);
                if let Some(project_index) = project_index {
                    let project = self.state.read(cx).projects[project_index].clone();
                    project.update(cx, |project, cx| {
                        project.chats = chats;
                        project.threads_loaded = true;
                        cx.notify();
                    });
                    if project_index == self.state.read(cx).active_project {
                        self.state.update(cx, |state, cx| {
                            state.active_chat = 0;
                            cx.notify();
                        });
                    }
                }
                self.set_bridge_status("connected to codex app-server", cx);
                let can_resume_default = !self.ui_state.read(cx).new_chat_open
                    && project_index == Some(self.state.read(cx).active_project);
                if can_resume_default {
                    if let Some(thread_id) = default_thread_id {
                        self.send_bridge(BridgeCommand::ResumeThread { thread_id }, cx);
                        self.set_bridge_status("loading thread", cx);
                    }
                }
            }
            BridgeEvent::ModelsLoaded(models) => {
                self.state.update(cx, |state, cx| {
                    if let Some(default_model) = models
                        .first()
                        .filter(|_| state.chat_settings.model.is_empty())
                    {
                        state.chat_settings.model = default_model.id.clone();
                        state.chat_settings.effort = default_model.default_effort.clone();
                    }
                    state.available_models = models;
                    cx.notify();
                });
            }
            BridgeEvent::PermissionProfilesLoaded(profiles) => {
                self.state.update(cx, |state, cx| {
                    state.permission_profiles = profiles;
                    cx.notify();
                });
            }
            BridgeEvent::ThreadStarted(thread) | BridgeEvent::ThreadForked(thread) => {
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
                    self.send_bridge(
                        BridgeCommand::SendTurn {
                            thread_id,
                            text,
                            settings,
                        },
                        cx,
                    );
                    self.set_bridge_status("turn running", cx);
                }
            }
            BridgeEvent::ThreadResumed(thread) => {
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
            BridgeEvent::TurnStarted { thread_id } => {
                let _ = thread_id;
                self.set_bridge_status("turn running", cx);
                // self.append_message(
                //     &thread_id,
                //     Message::Commentary("Codex accepted the turn.".into()),
                //     cx,
                // );
            }
            BridgeEvent::ItemStarted { thread_id, item } => {
                self.apply_item_started(&thread_id, item, cx);
            }
            BridgeEvent::AgentMessageDelta {
                thread_id,
                item_id,
                delta,
            } => self.append_agent_delta(&thread_id, &item_id, &delta, cx),
            BridgeEvent::ToolOutputDelta {
                thread_id,
                item_id,
                delta,
            } => self.append_tool_output_delta(&thread_id, &item_id, &delta, cx),
            BridgeEvent::ItemCompleted { thread_id, item } => {
                self.apply_item_completed(&thread_id, item, cx);
            }
            BridgeEvent::Error(message) => {
                self.set_bridge_status("codex app-server error", cx);
                if let Some(chat) = self.active_chat_entity(cx) {
                    let thread_id = chat.read(cx).id.clone();
                    self.append_message(&thread_id, Message::Commentary(message), cx);
                } else if let Some(project) = self.active_project_entity(cx) {
                    let chat = cx.new(|cx| {
                        ChatState::new(
                            "bridge-error".into(),
                            "Bridge error".into(),
                            message.clone().into(),
                            vec![cx.new(|_| MessageState::new(Message::Commentary(message)))],
                        )
                    });
                    project.update(cx, |project, cx| {
                        project.chats.push(chat);
                        cx.notify();
                    });
                }
            }
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
            ThreadItem::AgentMessage { id, text, .. } => {
                self.append_message(
                    thread_id,
                    Message::Assistant {
                        id,
                        body: text,
                        state: StreamState::Streaming,
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
                    ToolCall {
                        id,
                        name: tool_name(&command, "command"),
                        status: ToolStatus::Running,
                        detail: cwd.display().to_string(),
                    },
                    cx,
                );
            }
            ThreadItem::McpToolCall { id, tool, .. }
            | ThreadItem::DynamicToolCall { id, tool, .. } => {
                self.append_or_update_tool(
                    thread_id,
                    ToolCall {
                        id,
                        name: tool_name(&tool, "tool call"),
                        status: ToolStatus::Running,
                        detail: "tool call started".into(),
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
                aggregated_output,
                status,
                ..
            } => {
                self.append_or_update_tool(
                    thread_id,
                    ToolCall {
                        id: id.clone(),
                        name: tool_name(&command, "command"),
                        status: tool_status_from_command(&status),
                        detail: aggregated_output.unwrap_or_else(|| cwd.display().to_string()),
                    },
                    cx,
                );
                self.mark_item_complete(thread_id, &id, cx);
            }
            ThreadItem::McpToolCall {
                id, tool, status, ..
            } => {
                self.append_or_update_tool(
                    thread_id,
                    ToolCall {
                        id: id.clone(),
                        name: tool_name(&tool, "tool call"),
                        status: tool_status_from_mcp(&status),
                        detail: String::new(),
                    },
                    cx,
                );
                self.mark_item_complete(thread_id, &id, cx);
            }
            ThreadItem::DynamicToolCall {
                id, tool, status, ..
            } => {
                self.append_or_update_tool(
                    thread_id,
                    ToolCall {
                        id: id.clone(),
                        name: tool_name(&tool, "tool call"),
                        status: tool_status_from_dynamic(&status),
                        detail: String::new(),
                    },
                    cx,
                );
                self.mark_item_complete(thread_id, &id, cx);
            }
            item => self.mark_item_complete(thread_id, item.id(), cx),
        }
    }

    fn active_project_entity(&self, cx: &mut Context<Self>) -> Option<Entity<ProjectState>> {
        self.state.read(cx).active_project()
    }

    fn active_chat_entity(&self, cx: &mut Context<Self>) -> Option<Entity<ChatState>> {
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
        let message = cx.new(|_| MessageState::new(message));
        chat.update(cx, |chat, cx| {
            chat.messages.push(message);
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
                cx.notify();
            }
        });
    }

    fn append_or_update_tool(&self, thread_id: &str, tool: ToolCall, cx: &mut Context<Self>) {
        if let Some(message) = self.find_latest_assistant_message(thread_id, cx) {
            message.update(cx, |message, cx| {
                if let Message::Assistant { tools, .. } = &mut message.message {
                    if let Some(existing) = tools.iter_mut().find(|existing| existing.id == tool.id)
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
                    id: format!("tool-group-{}", tool.id),
                    body: "Codex is using tools.".into(),
                    state: StreamState::Streaming,
                    tools: vec![tool],
                },
                cx,
            );
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
                if let Some(tool) = tools.iter_mut().find(|tool| tool.id == item_id) {
                    tool.detail.push_str(delta);
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
                    cx.notify();
                }
            });
            return;
        }

        if let Some(message) = self.find_tool_message(thread_id, item_id, cx) {
            message.update(cx, |message, cx| {
                if let Message::Assistant { tools, .. } = &mut message.message {
                    if let Some(tool) = tools.iter_mut().find(|tool| tool.id == item_id) {
                        tool.status = ToolStatus::Done;
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

    fn find_latest_assistant_message(
        &self,
        thread_id: &str,
        cx: &mut Context<Self>,
    ) -> Option<Entity<MessageState>> {
        let chat = self.find_chat_entity(thread_id, cx)?;
        let messages = chat.read(cx).messages.clone();
        for message in messages.into_iter().rev() {
            let is_match = matches!(&message.read(cx).message, Message::Assistant { .. });
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
                Message::Assistant { tools, .. } if tools.iter().any(|tool| tool.id == item_id)
            );
            if is_match {
                return Some(message);
            }
        }
        None
    }
}

impl Render for CodexGui {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.drain_bridge_events(cx);
        let side_chat_open = self.ui_state.read(cx).side_chat_open;

        div()
            .size_full()
            .bg(transparent_black())
            .text_color(cx.theme().foreground)
            .font_family(".SystemUIFont")
            .child(
                div().relative().size_full().child(
                    div()
                        .flex()
                        .size_full()
                        .child(self.sidebar.clone())
                        .child(self.chat_panel.clone())
                        .when(side_chat_open, |this| this.child(self.side_chat.clone())),
                ),
            )
    }
}

fn chat_entity_from_thread(thread: Thread, cx: &mut Context<CodexGui>) -> Entity<ChatState> {
    let title = thread
        .name
        .as_deref()
        .filter(|name| !name.is_empty())
        .or_else(|| Some(thread.preview.as_str()))
        .filter(|preview| !preview.is_empty())
        .unwrap_or("Untitled Codex thread")
        .to_string();
    let subtitle = format!(
        "{} - {}",
        thread_status_label(&thread.status),
        thread.cwd.display()
    );
    let thread_id = thread.id;
    let mut messages = Vec::new();
    for turn in thread.turns {
        for item in turn.items {
            append_thread_item(&mut messages, item, cx);
        }
    }
    cx.new(|_| ChatState::new(thread_id, title.into(), subtitle.into(), messages))
}

fn empty_chat_entity(cx: &mut Context<CodexGui>) -> Entity<ChatState> {
    cx.new(|cx| {
        ChatState::new(
            "empty".into(),
            "No Codex threads".into(),
            "Click New to start one in this workspace".into(),
            vec![cx.new(|_| {
                MessageState::new(Message::Commentary(
                    "No persisted Codex threads were returned for this workspace.".into(),
                ))
            })],
        )
    })
}

fn project_name_from_path(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
        .to_string()
}

fn append_thread_item(
    messages: &mut Vec<Entity<MessageState>>,
    item: ThreadItem,
    cx: &mut Context<CodexGui>,
) {
    match item {
        ThreadItem::UserMessage { content, .. } => {
            let text = user_input_text(&content);
            if !text.is_empty() {
                messages.push(cx.new(|_| MessageState::new(Message::User(text))));
            }
        }
        ThreadItem::AgentMessage { id, text, .. } => {
            messages.push(cx.new(|_| {
                MessageState::new(Message::Assistant {
                    id,
                    body: text,
                    state: StreamState::Complete,
                    tools: Vec::new(),
                })
            }));
        }
        ThreadItem::CommandExecution {
            id,
            command,
            cwd,
            aggregated_output,
            status,
            ..
        } => {
            push_tool_to_messages(
                messages,
                ToolCall {
                    id,
                    name: tool_name(&command, "command"),
                    status: tool_status_from_command(&status),
                    detail: aggregated_output.unwrap_or_else(|| cwd.display().to_string()),
                },
                cx,
            );
        }
        ThreadItem::McpToolCall {
            id, tool, status, ..
        } => {
            push_tool_to_messages(
                messages,
                ToolCall {
                    id,
                    name: tool_name(&tool, "tool call"),
                    status: tool_status_from_mcp(&status),
                    detail: String::new(),
                },
                cx,
            );
        }
        ThreadItem::DynamicToolCall {
            id, tool, status, ..
        } => {
            push_tool_to_messages(
                messages,
                ToolCall {
                    id,
                    name: tool_name(&tool, "tool call"),
                    status: tool_status_from_dynamic(&status),
                    detail: String::new(),
                },
                cx,
            );
        }
        _ => {}
    }
}

fn push_tool_to_messages(
    messages: &mut Vec<Entity<MessageState>>,
    tool: ToolCall,
    cx: &mut Context<CodexGui>,
) {
    for message in messages.iter().rev() {
        let is_assistant = matches!(&message.read(cx).message, Message::Assistant { .. });
        if is_assistant {
            message.update(cx, |message, cx| {
                if let Message::Assistant { tools, .. } = &mut message.message {
                    tools.push(tool);
                    cx.notify();
                }
            });
            return;
        }
    }

    messages.push(cx.new(|_| {
        MessageState::new(Message::Assistant {
            id: format!("tool-group-{}", tool.id),
            body: "Codex used a tool.".into(),
            state: StreamState::Complete,
            tools: vec![tool],
        })
    }));
}

fn upsert_chat(
    project: &mut ProjectState,
    chat: Entity<ChatState>,
    chat_id: &str,
    cx: &mut Context<ProjectState>,
) {
    for index in 0..project.chats.len() {
        let is_match = project.chats[index].read(cx).id == chat_id;
        if is_match {
            project.chats[index] = chat;
            return;
        }
    }
    project.chats.insert(0, chat);
}

fn thread_status_label(status: &ThreadStatus) -> &'static str {
    match status {
        ThreadStatus::NotLoaded => "not loaded",
        ThreadStatus::Idle => "idle",
        ThreadStatus::SystemError => "system error",
        ThreadStatus::Active { .. } => "active",
    }
}

fn tool_status_from_command(status: &CommandExecutionStatus) -> ToolStatus {
    match status {
        CommandExecutionStatus::InProgress => ToolStatus::Running,
        CommandExecutionStatus::Completed
        | CommandExecutionStatus::Failed
        | CommandExecutionStatus::Declined => ToolStatus::Done,
    }
}

fn tool_status_from_mcp(status: &McpToolCallStatus) -> ToolStatus {
    match status {
        McpToolCallStatus::InProgress => ToolStatus::Running,
        McpToolCallStatus::Completed | McpToolCallStatus::Failed => ToolStatus::Done,
    }
}

fn tool_status_from_dynamic(status: &DynamicToolCallStatus) -> ToolStatus {
    match status {
        DynamicToolCallStatus::InProgress => ToolStatus::Running,
        DynamicToolCallStatus::Completed | DynamicToolCallStatus::Failed => ToolStatus::Done,
    }
}

fn tool_name(name: &str, fallback: &str) -> String {
    if name.is_empty() {
        fallback.into()
    } else {
        name.into()
    }
}

fn user_input_text(content: &[UserInput]) -> String {
    content
        .iter()
        .filter_map(|input| match input {
            UserInput::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}
