//! Read RPC responses and update view state accordingly

use super::{
    CodexGui,
    thread_mapping::{chat_entity_from_thread, empty_chat_entity},
};
use crate::{
    bridge::BridgeError,
    gui::{ModelOption, PermissionProfileOption},
};
use codex_app_server_protocol::Thread;
use gpui::Context;

impl CodexGui {
    pub(super) fn set_bridge_status(&self, status: impl Into<String>, cx: &mut Context<Self>) {
        self.bridge_state.update(cx, |state, cx| {
            state.set_status(status);
            cx.notify();
        });
    }

    pub(super) fn apply_initialize_result(
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

    pub(super) fn apply_threads_result(
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
            .update(cx, |state, cx| state.project_index_by_path(&cwd, cx));
        if let Some(project_index) = project_index {
            let project = self.state.read(cx).projects[project_index].clone();
            project.update(cx, |project, cx| {
                project.replace_loaded_chats(chats);
                cx.notify();
            });
            if project_index == self.state.read(cx).active_project {
                self.state.update(cx, |state, cx| {
                    state.select_first_chat();
                    cx.notify();
                });
            }
        }

        self.set_bridge_status("connected to codex app-server", cx);
        let can_resume_default = !self.ui_state.read(cx).new_chat_open
            && project_index == Some(self.state.read(cx).active_project);
        if can_resume_default && let Some(thread_id) = default_thread_id {
            self.request_resume_thread(thread_id, cx);
            self.set_bridge_status("loading thread", cx);
        }
    }

    pub(super) fn apply_models_result(
        &mut self,
        result: Result<Vec<ModelOption>, BridgeError>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(models) => {
                self.state.update(cx, |state, cx| {
                    state.set_available_models(models);
                    cx.notify();
                });
            }
            Err(err) => self.apply_bridge_error(err.to_string(), cx),
        }
    }

    pub(super) fn apply_permission_profiles_result(
        &mut self,
        result: Result<Vec<PermissionProfileOption>, BridgeError>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(profiles) => {
                self.state.update(cx, |state, cx| {
                    state.set_permission_profiles(profiles);
                    cx.notify();
                });
            }
            Err(err) => self.apply_bridge_error(err.to_string(), cx),
        }
    }

    pub(super) fn apply_thread_started_result(
        &mut self,
        result: Result<Thread, BridgeError>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(thread) => self.apply_thread_started(thread, cx),
            Err(err) => self.apply_bridge_error(err.to_string(), cx),
        }
    }

    pub(super) fn apply_thread_resumed_result(
        &mut self,
        result: Result<Thread, BridgeError>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(thread) => self.apply_thread_resumed(thread, cx),
            Err(err) => self.apply_bridge_error(err.to_string(), cx),
        }
    }

    pub(super) fn apply_unit_result(
        &mut self,
        result: Result<(), BridgeError>,
        cx: &mut Context<Self>,
    ) {
        if let Err(err) = result {
            self.apply_bridge_error(err.to_string(), cx);
        }
    }
}
