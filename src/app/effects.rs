//! Send bridge requests

use super::CodexGui;
use crate::gui::ChatSettings;
use gpui::{AppContext, Context};

impl CodexGui {
    pub(super) fn request_project_threads(&self, cwd: String, cx: &mut Context<Self>) {
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

    pub(super) fn request_start_thread(
        &self,
        cwd: String,
        settings: ChatSettings,
        cx: &mut Context<Self>,
    ) {
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { bridge.start_thread(cwd, settings).await })
                .await;
            let _ = this.update(cx, |view, cx| view.apply_thread_started_result(result, cx));
        })
        .detach();
    }

    pub(super) fn request_resume_thread(&self, thread_id: String, cx: &mut Context<Self>) {
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { bridge.resume_thread(thread_id).await })
                .await;
            let _ = this.update(cx, |view, cx| view.apply_thread_resumed_result(result, cx));
        })
        .detach();
    }

    pub(super) fn request_fork_thread(&self, thread_id: String, cx: &mut Context<Self>) {
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { bridge.fork_thread(thread_id).await })
                .await;
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
            let result = cx
                .background_spawn(async move { bridge.send_turn(thread_id, text, settings).await })
                .await;
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
            let result = cx
                .background_spawn(async move { bridge.steer_turn(thread_id, turn_id, text).await })
                .await;
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
            let result = cx
                .background_spawn(async move { bridge.interrupt_turn(thread_id, turn_id).await })
                .await;
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
            let result = cx
                .background_spawn(async move {
                    bridge.update_thread_settings(thread_id, settings).await
                })
                .await;
            let _ = this.update(cx, |view, cx| view.apply_unit_result(result, cx));
        })
        .detach();
    }
}
