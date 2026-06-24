//! Handle user intents

use super::{
    CodexGui,
    thread_mapping::{project_name_from_path, should_start_thread_for_turn},
};
use crate::{
    gui::{ApprovalReviewerMode, ProjectState},
    workspace::workspace_path,
};
use gpui::{AppContext, Context};

impl CodexGui {
    pub(crate) fn select_project(&mut self, index: usize, cx: &mut Context<Self>) {
        let (cwd, should_load_threads) = self
            .state
            .read(cx)
            .projects
            .get(index)
            .map(|project| {
                let project = project.read(cx);
                (project.path.to_string(), !project.threads_loaded)
            })
            .unwrap_or_else(|| (workspace_path(), false));

        self.state.update(cx, |state, cx| {
            state.select_project(index);
            cx.notify();
        });

        if should_load_threads {
            self.request_project_threads(cwd, cx);
            self.set_bridge_status("loading project threads", cx);
        }
    }

    pub(crate) fn open_new_chat(&mut self, cx: &mut Context<Self>) {
        self.ui_state.update(cx, |state, cx| {
            state.open_new_chat();
            cx.notify();
        });
        cx.notify();
    }

    pub(crate) fn add_project(&mut self, path: String, cx: &mut Context<Self>) {
        let path = path.trim();
        if path.is_empty() {
            return;
        }

        if let Some(index) = self
            .state
            .update(cx, |state, cx| state.project_index_by_path(path, cx))
        {
            self.select_project(index, cx);
            self.open_new_chat(cx);
            return;
        }

        let name = project_name_from_path(path);
        let project = cx.new(|_| ProjectState::new(name.into(), path.into(), Vec::new()));
        let index = self.state.update(cx, |state, cx| {
            let index = state.add_project(project);
            cx.notify();
            index
        });
        self.open_new_chat(cx);
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
            state.select_chat(index);
            cx.notify();
        });
        self.ui_state.update(cx, |state, cx| {
            state.close_new_chat();
            cx.notify();
        });

        if let Some(thread_id) = thread_id.filter(|thread_id| thread_id != "empty") {
            self.request_resume_thread(thread_id, cx);
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
        self.request_fork_thread(thread_id, cx);
        self.set_bridge_status("forking thread", cx);
    }

    /// Starts an empty thread for the active project.
    ///
    /// Composer submission normally goes through `submit_turn_text` so the first
    /// prompt can be sent after the asynchronous thread creation completes.
    pub(crate) fn start_new_thread(&mut self, cx: &mut Context<Self>) {
        let settings = self.state.read(cx).chat_settings.clone();
        let cwd = self
            .active_project_entity(cx)
            .map(|project| project.read(cx).path.to_string())
            .unwrap_or_else(workspace_path);
        self.request_start_thread(cwd, settings, cx);
        self.set_bridge_status("starting thread", cx);
    }

    /// Handles a composer submit.
    ///
    /// If the UI is on the new-chat page, this stashes the text, creates a
    /// thread, and lets `apply_thread_started` send the pending first turn.
    pub(crate) fn submit_turn_text(&mut self, text: String, cx: &mut Context<Self>) {
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
            self.start_new_thread(cx);
            return;
        }

        let Some(thread_id) = active_thread_id else {
            return;
        };
        let settings = self.state.read(cx).chat_settings.clone();
        self.request_send_turn(thread_id, text, settings, cx);
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
        self.request_steer_turn(active_thread_id, active_turn.turn_id, text, cx);
        self.set_bridge_status("steer sent", cx);
    }

    pub(crate) fn stop_active_turn(&mut self, cx: &mut Context<Self>) {
        let Some(active_turn) = self.ui_state.read(cx).active_turn.clone() else {
            return;
        };
        self.request_interrupt_turn(active_turn.thread_id, active_turn.turn_id, cx);
        self.set_bridge_status("stopping turn", cx);
    }

    pub(crate) fn set_model(&mut self, model: String, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| {
            state.set_model(model);
            cx.notify();
        });
        self.sync_active_thread_settings(cx);
    }

    pub(crate) fn set_effort(&mut self, effort: String, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| {
            state.set_effort(effort);
            cx.notify();
        });
        self.sync_active_thread_settings(cx);
    }

    pub(crate) fn set_permission_profile(
        &mut self,
        permission_profile: String,
        cx: &mut Context<Self>,
    ) {
        self.state.update(cx, |state, cx| {
            state.set_permission_profile(permission_profile);
            cx.notify();
        });
        self.sync_active_thread_settings(cx);
    }

    pub(crate) fn set_approvals_reviewer(
        &mut self,
        approvals_reviewer: ApprovalReviewerMode,
        cx: &mut Context<Self>,
    ) {
        self.state.update(cx, |state, cx| {
            state.set_approvals_reviewer(approvals_reviewer);
            cx.notify();
        });
        self.sync_active_thread_settings(cx);
    }

    pub(crate) fn toggle_side_chat(&mut self, cx: &mut Context<Self>) {
        self.ui_state.update(cx, |state, cx| {
            state.toggle_side_chat();
            cx.notify();
        });
        cx.notify();
    }

    fn sync_active_thread_settings(&mut self, cx: &mut Context<Self>) {
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
        self.request_update_thread_settings(thread_id, settings, cx);
        self.set_bridge_status("updating settings", cx);
    }
}
