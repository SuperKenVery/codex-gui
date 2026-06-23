use crate::bridge::{AppServerBridge, BridgeError, BridgeEvent, start_app_server_bridge};
use crate::gui::{
    ActiveTurn, ApprovalReviewerMode, AssistantPhase, BridgeState, ChatPanel, ChatSettings,
    ChatState, GuiState, Message, MessageState, ModelOption, PermissionProfileOption, ProjectState,
    SideChat, Sidebar, StreamState, ToolCall, ToolStatus, UiState,
};
use crate::workspace::workspace_path;
use codex_app_server_protocol::{
    CommandExecutionStatus, DynamicToolCallStatus, McpToolCallStatus, ServerNotification, Thread,
    ThreadItem, ThreadStatus, UserInput,
};
use codex_protocol::models::MessagePhase;
use gpui::{
    Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Task, Window, div,
    prelude::*, transparent_black,
};
use gpui_component::ActiveTheme as _;
use std::{path::Path, sync::mpsc::Receiver, time::Duration};

pub struct CodexGui {
    state: Entity<GuiState>,
    ui_state: Entity<UiState>,
    bridge_state: Entity<BridgeState>,
    bridge: AppServerBridge,
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
        let (bridge, bridge_rx) = start_app_server_bridge();
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

        let initialize_bridge = bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { initialize_bridge.initialize().await })
                .await;
            let _ = this.update(cx, |view, cx| view.apply_initialize_result(result, cx));
        })
        .detach();

        let models_bridge = bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { models_bridge.list_models().await })
                .await;
            let _ = this.update(cx, |view, cx| view.apply_models_result(result, cx));
        })
        .detach();

        let profiles_bridge = bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    profiles_bridge
                        .list_permission_profiles(workspace_path())
                        .await
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.apply_permission_profiles_result(result, cx)
            });
        })
        .detach();

        let threads_bridge = bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let cwd = workspace_path();
                    threads_bridge
                        .list_threads(cwd.clone())
                        .await
                        .map(|threads| (cwd, threads))
                })
                .await;
            let _ = this.update(cx, |view, cx| view.apply_threads_result(result, cx));
        })
        .detach();

        Self {
            state,
            ui_state,
            bridge_state,
            bridge,
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
                self.load_threads(cwd, cx);
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
            self.resume_thread(thread_id, cx);
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
        self.fork_thread(thread_id, cx);
        self.set_bridge_status("forking thread", cx);
    }

    pub(crate) fn start_thread(&mut self, cx: &mut Context<Self>) {
        let settings = self.state.read(cx).chat_settings.clone();
        let cwd = self
            .active_project_entity(cx)
            .map(|project| project.read(cx).path.to_string())
            .unwrap_or_else(workspace_path);
        self.start_thread_request(cwd, settings, cx);
        self.set_bridge_status("starting thread", cx);
    }

    pub(crate) fn send_turn_text(&mut self, text: String, cx: &mut Context<Self>) {
        if self.ui_state.read(cx).active_turn.is_some() {
            return;
        }
        let active_thread_id = self
            .active_chat_entity(cx)
            .map(|chat| chat.read(cx).id.clone())
            .filter(|thread_id| thread_id != "empty");
        let new_chat_open = self.ui_state.read(cx).new_chat_open;
        if should_start_thread_for_turn(new_chat_open, active_thread_id.as_deref()) {
            self.pending_turn_text = Some(text);
            self.start_thread(cx);
            return;
        }
        let Some(thread_id) = active_thread_id else {
            return;
        };
        let settings = self.state.read(cx).chat_settings.clone();
        self.send_turn_request(thread_id, text, settings, cx);
        self.set_bridge_status("turn running", cx);
    }

    pub(crate) fn steer_turn_text(&mut self, text: String, cx: &mut Context<Self>) {
        let Some(active_turn) = self.ui_state.read(cx).active_turn.clone() else {
            return;
        };
        let Some(active_thread_id) = self
            .active_chat_entity(cx)
            .map(|chat| chat.read(cx).id.clone())
            .filter(|thread_id| thread_id == &active_turn.thread_id)
        else {
            return;
        };
        self.steer_turn_request(active_thread_id, active_turn.turn_id, text, cx);
        self.set_bridge_status("steer sent", cx);
    }

    pub(crate) fn stop_active_turn(&mut self, cx: &mut Context<Self>) {
        let Some(active_turn) = self.ui_state.read(cx).active_turn.clone() else {
            return;
        };
        self.interrupt_turn_request(active_turn.thread_id, active_turn.turn_id, cx);
        self.set_bridge_status("stopping turn", cx);
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

    fn update_active_thread_settings(&mut self, cx: &mut Context<Self>) {
        if self.ui_state.read(cx).new_chat_open {
            return;
        }
        let Some(thread_id) = self
            .active_chat_entity(cx)
            .map(|chat| chat.read(cx).id.clone())
            .filter(|thread_id| thread_id != "empty" && thread_id != "bridge-error")
        else {
            return;
        };
        let settings = self.state.read(cx).chat_settings.clone();
        self.update_thread_settings_request(thread_id, settings, cx);
        self.set_bridge_status("updating settings", cx);
    }

    fn load_threads(&self, cwd: String, cx: &mut Context<Self>) {
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    bridge
                        .list_threads(cwd.clone())
                        .await
                        .map(|threads| (cwd, threads))
                })
                .await;
            let _ = this.update(cx, |view, cx| view.apply_threads_result(result, cx));
        })
        .detach();
    }

    fn start_thread_request(&self, cwd: String, settings: ChatSettings, cx: &mut Context<Self>) {
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { bridge.start_thread(cwd, settings).await })
                .await;
            let _ = this.update(cx, |view, cx| view.apply_thread_started_result(result, cx));
        })
        .detach();
    }

    fn resume_thread(&self, thread_id: String, cx: &mut Context<Self>) {
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { bridge.resume_thread(thread_id).await })
                .await;
            let _ = this.update(cx, |view, cx| view.apply_thread_resumed_result(result, cx));
        })
        .detach();
    }

    fn fork_thread(&self, thread_id: String, cx: &mut Context<Self>) {
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { bridge.fork_thread(thread_id).await })
                .await;
            let _ = this.update(cx, |view, cx| view.apply_thread_started_result(result, cx));
        })
        .detach();
    }

    fn send_turn_request(
        &self,
        thread_id: String,
        text: String,
        settings: ChatSettings,
        cx: &mut Context<Self>,
    ) {
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { bridge.send_turn(thread_id, text, settings).await })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.apply_unit_result(result.map(|_| ()), cx)
            });
        })
        .detach();
    }

    fn steer_turn_request(
        &self,
        thread_id: String,
        turn_id: String,
        text: String,
        cx: &mut Context<Self>,
    ) {
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { bridge.steer_turn(thread_id, turn_id, text).await })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.apply_unit_result(result.map(|_| ()), cx)
            });
        })
        .detach();
    }

    fn interrupt_turn_request(&self, thread_id: String, turn_id: String, cx: &mut Context<Self>) {
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { bridge.interrupt_turn(thread_id, turn_id).await })
                .await;
            let _ = this.update(cx, |view, cx| view.apply_unit_result(result, cx));
        })
        .detach();
    }

    fn update_thread_settings_request(
        &self,
        thread_id: String,
        settings: ChatSettings,
        cx: &mut Context<Self>,
    ) {
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    bridge.update_thread_settings(thread_id, settings).await
                })
                .await;
            let _ = this.update(cx, |view, cx| view.apply_unit_result(result, cx));
        })
        .detach();
    }

    fn set_bridge_status(&self, status: impl Into<String>, cx: &mut Context<Self>) {
        self.bridge_state.update(cx, |state, cx| {
            state.status = status.into();
            cx.notify();
        });
    }

    fn apply_initialize_result(
        &mut self,
        result: Result<codex_app_server_protocol::InitializeResponse, BridgeError>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(response) => {
                self.set_bridge_status(format!("connected: {}", response.user_agent), cx)
            }
            Err(err) => self.apply_bridge_error(err.to_string(), cx),
        }
    }

    fn apply_threads_result(
        &mut self,
        result: Result<(String, Vec<Thread>), BridgeError>,
        cx: &mut Context<Self>,
    ) {
        let (cwd, threads) = match result {
            Ok(result) => result,
            Err(err) => {
                self.apply_bridge_error(err.to_string(), cx);
                return;
            }
        };

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
                self.resume_thread(thread_id, cx);
                self.set_bridge_status("loading thread", cx);
            }
        }
    }

    fn apply_models_result(
        &mut self,
        result: Result<Vec<ModelOption>, BridgeError>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(models) => {
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
            Err(err) => self.apply_bridge_error(err.to_string(), cx),
        }
    }

    fn apply_permission_profiles_result(
        &mut self,
        result: Result<Vec<PermissionProfileOption>, BridgeError>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(profiles) => {
                self.state.update(cx, |state, cx| {
                    state.permission_profiles = profiles;
                    cx.notify();
                });
            }
            Err(err) => self.apply_bridge_error(err.to_string(), cx),
        }
    }

    fn apply_thread_started_result(
        &mut self,
        result: Result<Thread, BridgeError>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(thread) => self.apply_thread_started(thread, cx),
            Err(err) => self.apply_bridge_error(err.to_string(), cx),
        }
    }

    fn apply_thread_resumed_result(
        &mut self,
        result: Result<Thread, BridgeError>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(thread) => self.apply_thread_resumed(thread, cx),
            Err(err) => self.apply_bridge_error(err.to_string(), cx),
        }
    }

    fn apply_unit_result(&mut self, result: Result<(), BridgeError>, cx: &mut Context<Self>) {
        if let Err(err) = result {
            self.apply_bridge_error(err.to_string(), cx);
        }
    }

    fn drain_bridge_events(&mut self, cx: &mut Context<Self>) {
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

    fn apply_thread_started(&mut self, thread: Thread, cx: &mut Context<Self>) {
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

    fn apply_thread_resumed(&mut self, thread: Thread, cx: &mut Context<Self>) {
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

    fn apply_bridge_error(&mut self, message: String, cx: &mut Context<Self>) {
        self.ui_state.update(cx, |state, cx| {
            state.active_turn = None;
            cx.notify();
        });
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
                    vec![cx.new(|cx| MessageState::new(Message::Commentary(message), cx))],
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
                    .all(|tool| matches!(tool.status, ToolStatus::Done))
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
                    message.sync_body_view(cx);
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
            vec![cx.new(|cx| {
                MessageState::new(
                    Message::Commentary(
                        "No persisted Codex threads were returned for this workspace.".into(),
                    ),
                    cx,
                )
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
                messages.push(cx.new(|cx| MessageState::new(Message::User(text), cx)));
            }
        }
        ThreadItem::AgentMessage {
            id, text, phase, ..
        } => {
            messages.push(cx.new(|cx| {
                MessageState::new(
                    Message::Assistant {
                        id,
                        body: text,
                        state: StreamState::Complete,
                        phase: assistant_phase(phase.as_ref()),
                        tools: Vec::new(),
                    },
                    cx,
                )
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

    messages.push(cx.new(|cx| {
        MessageState::new(
            Message::Assistant {
                id: format!("tool-group-{}", tool.id),
                body: String::new(),
                state: StreamState::Complete,
                phase: AssistantPhase::Commentary,
                tools: vec![tool],
            },
            cx,
        )
    }));
}

fn assistant_phase(phase: Option<&MessagePhase>) -> AssistantPhase {
    match phase {
        Some(MessagePhase::Commentary) => AssistantPhase::Commentary,
        Some(MessagePhase::FinalAnswer) | None => AssistantPhase::FinalAnswer,
    }
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

fn should_start_thread_for_turn(new_chat_open: bool, active_thread_id: Option<&str>) -> bool {
    new_chat_open || active_thread_id.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_chat_turn_starts_thread_even_when_an_active_thread_exists() {
        assert!(should_start_thread_for_turn(
            true,
            Some("existing-thread-id")
        ));
    }

    #[test]
    fn existing_chat_turn_reuses_active_thread() {
        assert!(!should_start_thread_for_turn(
            false,
            Some("existing-thread-id")
        ));
    }

    #[test]
    fn missing_active_chat_starts_thread() {
        assert!(should_start_thread_for_turn(false, None));
    }
}
