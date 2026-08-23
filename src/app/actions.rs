//! Handle user intents

use super::{
    CodexGui,
    thread_mapping::{empty_chat_entity, project_name_from_path, should_start_thread_for_turn},
};
use crate::{
    gui::{ApprovalReviewerMode, ProjectState},
    workspace::workspace_path,
};
use gpui::{AppContext, Context};

impl CodexGui {
    pub(crate) fn select_project(&mut self, index: usize, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| {
            state.select_project(index);
            cx.notify();
        });
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
        let empty_chat = empty_chat_entity(cx);
        let project = cx.new(|_| ProjectState::new(name.into(), path.into(), vec![empty_chat]));
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
            tracing::info!(thread_id, "loading thread");
            let bridge = self.bridge.clone();
            cx.spawn(async move |this, cx| {
                let result = bridge.resume_thread(thread_id).await;
                let _ = this.update(cx, |view, cx| view.apply_thread_resumed_result(result, cx));
            })
            .detach();
        }
    }

    pub(crate) fn select_projectless_chat(&mut self, index: usize, cx: &mut Context<Self>) {
        let thread_id = self
            .state
            .read(cx)
            .projectless_chats
            .get(index)
            .map(|chat| chat.read(cx).id.clone());

        self.state.update(cx, |state, cx| {
            state.select_projectless_chat(index);
            cx.notify();
        });
        self.ui_state.update(cx, |state, cx| {
            state.close_new_chat();
            cx.notify();
        });

        if let Some(thread_id) = thread_id.filter(|thread_id| thread_id != "empty") {
            tracing::info!(thread_id, "loading thread");
            let bridge = self.bridge.clone();
            cx.spawn(async move |this, cx| {
                let result = bridge.resume_thread(thread_id).await;
                let _ = this.update(cx, |view, cx| view.apply_thread_resumed_result(result, cx));
            })
            .detach();
        }
    }

    pub(crate) fn fork_chat(&mut self, cx: &mut Context<Self>) {
        let Some(thread_id) = self
            .active_chat_entity(cx)
            .map(|chat| chat.read(cx).id.clone())
        else {
            return;
        };
        tracing::info!(thread_id, "forking thread");
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = bridge.fork_thread(thread_id).await;
            let _ = this.update(cx, |view, cx| view.apply_thread_started_result(result, cx));
        })
        .detach();
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
        tracing::info!(cwd, "starting thread");
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = bridge.start_thread(cwd, settings).await;
            let _ = this.update(cx, |view, cx| view.apply_thread_started_result(result, cx));
        })
        .detach();
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
        tracing::info!(thread_id, "starting turn");
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = bridge
                .send_turn(thread_id, text, settings)
                .await
                .map(|_| ());
            let _ = this.update(cx, |view, cx| view.apply_unit_result(result, cx));
        })
        .detach();
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
        tracing::info!(
            thread_id = active_thread_id,
            turn_id = active_turn.turn_id,
            "steering turn"
        );
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = bridge
                .steer_turn(active_thread_id, active_turn.turn_id, text)
                .await
                .map(|_| ());
            let _ = this.update(cx, |view, cx| view.apply_unit_result(result, cx));
        })
        .detach();
    }

    pub(crate) fn stop_active_turn(&mut self, cx: &mut Context<Self>) {
        let Some(active_turn) = self.ui_state.read(cx).active_turn.clone() else {
            return;
        };
        tracing::info!(
            thread_id = active_turn.thread_id,
            turn_id = active_turn.turn_id,
            "stopping turn"
        );
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = bridge
                .interrupt_turn(active_turn.thread_id, active_turn.turn_id)
                .await;
            let _ = this.update(cx, |view, cx| view.apply_unit_result(result, cx));
        })
        .detach();
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
        tracing::info!(thread_id, "updating thread settings");
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = bridge.update_thread_settings(thread_id, settings).await;
            let _ = this.update(cx, |view, cx| view.apply_unit_result(result, cx));
        })
        .detach();
    }
}
