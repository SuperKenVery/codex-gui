use self::thread_mapping::{
    chat_entity_from_thread, empty_chat_entity, project_name_from_path,
    should_start_thread_for_turn,
};
use crate::bridge::{AppServerBridge, BridgeError, BridgeEvent, start_app_server_bridge};
use crate::gui::{
    ApprovalReviewerMode, BridgeState, ChatPanel, ChatSettings, GuiState, ModelOption,
    PermissionProfileOption, ProjectState, SideChat, Sidebar, UiState,
};
use crate::workspace::workspace_path;
use codex_app_server_protocol::Thread;
mod event_handler;
mod thread_mapping;

use gpui::{
    Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Task, Window, div,
    prelude::*, transparent_black,
};
use gpui_component::ActiveTheme as _;
use std::{sync::mpsc::Receiver, time::Duration};

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
