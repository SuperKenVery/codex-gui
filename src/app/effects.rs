//! Send bridge requests

use super::CodexGui;
use crate::{gui::ChatSettings, workspace::workspace_path};
use gpui::Context;

impl CodexGui {
    pub(super) fn request_startup_data(&self, cx: &mut Context<Self>) {
        self.request_models(cx);
        self.request_permission_profiles(workspace_path(), cx);
        self.request_threads(cx);
    }

    pub(super) fn request_models(&self, cx: &mut Context<Self>) {
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = bridge.list_models().await;
            let _ = this.update(cx, |view, cx| view.apply_models_result(result, cx));
        })
        .detach();
    }

    pub(super) fn request_permission_profiles(&self, cwd: String, cx: &mut Context<Self>) {
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = bridge.list_permission_profiles(cwd).await;
            let _ = this.update(cx, |view, cx| {
                view.apply_permission_profiles_result(result, cx)
            });
        })
        .detach();
    }

    pub(super) fn request_threads(&self, cx: &mut Context<Self>) {
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = bridge.list_threads().await;
            let _ = this.update(cx, |view, cx| view.apply_threads_result(result, cx));
        })
        .detach();
    }

    pub(super) fn request_start_thread(
        &self,
        cwd: String,
        settings: ChatSettings,
        cx: &mut Context<Self>,
    ) {
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = bridge.start_thread(cwd, settings).await;
            let _ = this.update(cx, |view, cx| view.apply_thread_started_result(result, cx));
        })
        .detach();
    }

    pub(super) fn request_resume_thread(&self, thread_id: String, cx: &mut Context<Self>) {
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = bridge.resume_thread(thread_id).await;
            let _ = this.update(cx, |view, cx| view.apply_thread_resumed_result(result, cx));
        })
        .detach();
    }

    pub(super) fn request_fork_thread(&self, thread_id: String, cx: &mut Context<Self>) {
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = bridge.fork_thread(thread_id).await;
            let _ = this.update(cx, |view, cx| view.apply_thread_started_result(result, cx));
        })
        .detach();
    }

    pub(super) fn request_send_turn(
        &self,
        thread_id: String,
        text: String,
        settings: ChatSettings,
        cx: &mut Context<Self>,
    ) {
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = bridge.send_turn(thread_id, text, settings).await;
            let _ = this.update(cx, |view, cx| {
                view.apply_unit_result(result.map(|_| ()), cx)
            });
        })
        .detach();
    }

    pub(super) fn request_steer_turn(
        &self,
        thread_id: String,
        turn_id: String,
        text: String,
        cx: &mut Context<Self>,
    ) {
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = bridge.steer_turn(thread_id, turn_id, text).await;
            let _ = this.update(cx, |view, cx| {
                view.apply_unit_result(result.map(|_| ()), cx)
            });
        })
        .detach();
    }

    pub(super) fn request_interrupt_turn(
        &self,
        thread_id: String,
        turn_id: String,
        cx: &mut Context<Self>,
    ) {
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = bridge.interrupt_turn(thread_id, turn_id).await;
            let _ = this.update(cx, |view, cx| view.apply_unit_result(result, cx));
        })
        .detach();
    }

    pub(super) fn request_update_thread_settings(
        &self,
        thread_id: String,
        settings: ChatSettings,
        cx: &mut Context<Self>,
    ) {
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = bridge.update_thread_settings(thread_id, settings).await;
            let _ = this.update(cx, |view, cx| view.apply_unit_result(result, cx));
        })
        .detach();
    }
}
