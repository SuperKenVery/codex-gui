//! Handle user intents

use super::{
    CodexGui,
    thread_mapping::{empty_chat_entity, project_name_from_path, should_start_thread_for_turn},
};
use crate::{
    gui::{ApprovalReviewerMode, ChatState, ProjectState, single_line_title},
    workspace::workspace_path,
};
use codex_app_server_protocol::RequestId;
use gpui::{AppContext, Context};

impl CodexGui {
    pub(crate) fn dismiss_notice(
        &mut self,
        chat_id: String,
        notice_id: String,
        cx: &mut Context<Self>,
    ) {
        self.update_chat(&chat_id, cx, |chat| chat.remove_notice(&notice_id));
    }

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
            project.read(cx).chats.get(index).and_then(|chat| {
                chat.read(cx)
                    .thread
                    .as_ref()
                    .map(|thread| thread.id.clone())
            })
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
            self.ui_state.update(cx, |state, cx| {
                state.begin_thread_load(thread_id.clone());
                cx.notify();
            });
            let bridge = self.bridge.clone();
            cx.spawn(async move |this, cx| {
                let result = bridge.resume_thread(thread_id.clone()).await;
                let _ = this.update(cx, |view, cx| {
                    view.apply_thread_resumed_result(&thread_id, result, cx)
                });
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
            self.ui_state.update(cx, |state, cx| {
                state.begin_thread_load(thread_id.clone());
                cx.notify();
            });
            let bridge = self.bridge.clone();
            cx.spawn(async move |this, cx| {
                let result = bridge.resume_thread(thread_id.clone()).await;
                let _ = this.update(cx, |view, cx| {
                    view.apply_thread_resumed_result(&thread_id, result, cx)
                });
            })
            .detach();
        }
    }

    pub(crate) fn fork_chat_through(&mut self, turn_id: String, cx: &mut Context<Self>) {
        let Some(thread_id) = self
            .active_chat_entity(cx)
            .map(|chat| chat.read(cx).id.clone())
        else {
            return;
        };
        tracing::info!(thread_id, turn_id, "forking thread through turn");
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = bridge.fork_thread(thread_id, Some(turn_id), None).await;
            let _ = this.update(cx, |view, cx| view.apply_thread_started_result(result, cx));
        })
        .detach();
    }

    pub(crate) fn submit_edited_turn_text(
        &mut self,
        source_thread_id: String,
        turn_id: String,
        previous_turn_id: Option<String>,
        client_user_message_id: String,
        text: String,
        cx: &mut Context<Self>,
    ) {
        if self.ui_state.read(cx).active_turn.is_some() {
            return;
        }
        let is_source_active = self
            .active_chat_entity(cx)
            .is_some_and(|chat| chat.read(cx).id == source_thread_id);
        if !is_source_active {
            return;
        }

        tracing::info!(
            thread_id = source_thread_id,
            last_turn_id = previous_turn_id,
            "replacing thread for edited message"
        );
        let settings = self.state.read(cx).chat_settings.clone();
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let _notification_mute = bridge.mute_thread_notifications();
            let before_turn_id = previous_turn_id.is_none().then_some(turn_id);
            let forked_thread = match bridge
                .fork_thread(source_thread_id.clone(), previous_turn_id, before_turn_id)
                .await
            {
                Ok(thread) => thread,
                Err(err) => {
                    let _ =
                        this.update(cx, |view, cx| view.apply_bridge_error(err.to_string(), cx));
                    return;
                }
            };
            let replacement_thread_id = forked_thread.id.clone();
            let replaced = this
                .update(cx, |view, cx| {
                    view.replace_thread_in_ui(&source_thread_id, forked_thread, cx)
                })
                .unwrap_or(false);
            if !replaced {
                return;
            }

            let pending_started = this
                .update(cx, |view, cx| {
                    view.active_chat_entity(cx)
                        .filter(|chat| chat.read(cx).id == replacement_thread_id)
                        .is_some_and(|chat| {
                            chat.update(cx, |chat, cx| {
                                let started = chat.begin_user_message(
                                    client_user_message_id.clone(),
                                    text.clone(),
                                );
                                if started {
                                    cx.notify();
                                }
                                started
                            })
                        })
                })
                .unwrap_or(false);
            if !pending_started {
                return;
            }

            match bridge.delete_thread(source_thread_id.clone()).await {
                Ok(()) => {
                    let _ = this.update(cx, |view, cx| {
                        view.remove_thread_from_ui(&source_thread_id, cx)
                    });
                }
                Err(err) => {
                    let _ =
                        this.update(cx, |view, cx| view.apply_bridge_error(err.to_string(), cx));
                }
            }

            let result = bridge
                .send_turn(
                    replacement_thread_id.clone(),
                    client_user_message_id.clone(),
                    text,
                    settings,
                )
                .await
                .map(|_| ());
            let _ = this.update(cx, |view, cx| {
                view.apply_user_submission_result(
                    &replacement_thread_id,
                    &client_user_message_id,
                    result,
                    cx,
                )
            });
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
            let _notification_mute = bridge.mute_thread_notifications();
            let result = bridge.start_thread(cwd, settings).await;
            let _ = this.update(cx, |view, cx| view.apply_thread_started_result(result, cx));
        })
        .detach();
    }

    /// Handles a composer submit.
    ///
    /// If the UI is on the new-chat page, this stashes the text, creates a
    /// thread, and lets `apply_thread_started` send the pending first turn.
    pub(crate) fn submit_turn_text(
        &mut self,
        client_user_message_id: String,
        text: String,
        cx: &mut Context<Self>,
    ) {
        if self.ui_state.read(cx).active_turn.is_some() || self.pending_thread_chat.is_some() {
            return;
        }

        let active_thread_id = self.active_chat_entity(cx).and_then(|chat| {
            chat.read(cx)
                .thread
                .as_ref()
                .map(|thread| thread.id.clone())
        });
        let new_chat_open = self.ui_state.read(cx).new_chat_open;

        if should_start_thread_for_turn(new_chat_open, active_thread_id.as_deref()) {
            let Some(project) = self.active_project_entity(cx) else {
                return;
            };
            let cwd = project.read(cx).path.to_string();
            let title = single_line_title(&text);
            let pending_chat_id = format!("pending-{client_user_message_id}");
            let pending_chat = cx.new(|_| {
                let mut chat = ChatState::new(
                    pending_chat_id,
                    title.into(),
                    format!("starting - {cwd}").into(),
                    Vec::new(),
                );
                chat.begin_user_message(client_user_message_id, text);
                chat
            });
            project.update(cx, |project, cx| {
                project.chats.retain(|chat| {
                    let chat = chat.read(cx);
                    chat.id.as_str() != "empty" && !chat.id.starts_with("pending-")
                });
                project.chats.insert(0, pending_chat.clone());
                cx.notify();
            });
            self.state.update(cx, |state, cx| {
                state.select_chat(0);
                cx.notify();
            });
            self.ui_state.update(cx, |state, cx| {
                state.close_new_chat();
                cx.notify();
            });
            self.pending_thread_chat = Some(pending_chat);
            self.start_new_thread(cx);
            return;
        }

        let Some(chat) = self.active_chat_entity(cx) else {
            return;
        };
        if chat.read(cx).user_message_is_sending() {
            return;
        }
        let thread_id = chat.read(cx).id.clone();
        let pending_started = chat.update(cx, |chat, cx| {
            let started = chat.begin_user_message(client_user_message_id.clone(), text.clone());
            if started {
                cx.notify();
            }
            started
        });
        if !pending_started {
            return;
        }
        let settings = self.state.read(cx).chat_settings.clone();
        tracing::info!(thread_id, "starting turn");
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = bridge
                .send_turn(
                    thread_id.clone(),
                    client_user_message_id.clone(),
                    text,
                    settings,
                )
                .await
                .map(|_| ());
            let _ = this.update(cx, |view, cx| {
                view.apply_user_submission_result(&thread_id, &client_user_message_id, result, cx)
            });
        })
        .detach();
    }

    pub(crate) fn steer_turn_text(
        &mut self,
        client_user_message_id: String,
        text: String,
        cx: &mut Context<Self>,
    ) {
        let Some(active_turn) = self.ui_state.read(cx).active_turn.clone() else {
            return;
        };
        let Some(chat) = self
            .active_chat_entity(cx)
            .filter(|chat| chat.read(cx).id == active_turn.thread_id)
        else {
            return;
        };
        if chat.read(cx).user_message_is_sending() {
            return;
        }
        let active_thread_id = chat.read(cx).id.clone();
        let pending_started = chat.update(cx, |chat, cx| {
            let started = chat.begin_user_message(client_user_message_id.clone(), text.clone());
            if started {
                cx.notify();
            }
            started
        });
        if !pending_started {
            return;
        }
        tracing::info!(
            thread_id = active_thread_id,
            turn_id = active_turn.turn_id,
            "steering turn"
        );
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = bridge
                .steer_turn(
                    active_thread_id.clone(),
                    active_turn.turn_id,
                    client_user_message_id.clone(),
                    text,
                )
                .await
                .map(|_| ());
            let _ = this.update(cx, |view, cx| {
                view.apply_user_submission_result(
                    &active_thread_id,
                    &client_user_message_id,
                    result,
                    cx,
                )
            });
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

    pub(crate) fn resolve_approval(
        &mut self,
        request_id: RequestId,
        approved: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.active_chat_entity(cx) else {
            return;
        };
        let thread_id = chat.read(cx).id.clone();
        let response = chat
            .read(cx)
            .pending_approvals
            .iter()
            .find(|approval| approval.request_id == request_id)
            .map(|approval| approval.response(approved));
        let Some(response) = response else {
            return;
        };
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                self.apply_bridge_error(format!("failed to encode approval response: {error}"), cx);
                return;
            }
        };

        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = bridge
                .resolve_server_request(request_id.clone(), response)
                .await;
            let _ = this.update(cx, |view, cx| match result {
                Ok(()) => view.remove_pending_approval(&thread_id, &request_id, cx),
                Err(error) => view.apply_thread_error(
                    &thread_id,
                    "approval",
                    format!("failed to resolve approval: {error}"),
                    false,
                    cx,
                ),
            });
        })
        .detach();
    }

    pub(crate) fn answer_server_input(
        &mut self,
        request_id: RequestId,
        question_id: String,
        answer: String,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.active_chat_entity(cx) else {
            return;
        };
        let thread_id = chat.read(cx).id.clone();
        let response = chat.update(cx, |chat, cx| {
            let response = chat.answer_input_request(&request_id, question_id, answer);
            cx.notify();
            response
        });
        let Some(response) = response else {
            return;
        };
        let response = match serde_json::to_value(response) {
            Ok(response) => response,
            Err(error) => {
                self.apply_thread_error(
                    &thread_id,
                    "user-input",
                    format!("failed to encode input response: {error}"),
                    false,
                    cx,
                );
                return;
            }
        };
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = bridge
                .resolve_server_request(request_id.clone(), response)
                .await;
            let _ = this.update(cx, |view, cx| match result {
                Ok(()) => view.remove_pending_input(&thread_id, &request_id, cx),
                Err(error) => view.apply_thread_error(
                    &thread_id,
                    "user-input",
                    format!("failed to answer input request: {error}"),
                    false,
                    cx,
                ),
            });
        })
        .detach();
    }

    pub(crate) fn reject_server_input(&mut self, request_id: RequestId, cx: &mut Context<Self>) {
        let Some(chat) = self.active_chat_entity(cx) else {
            return;
        };
        let thread_id = chat.read(cx).id.clone();
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = bridge
                .reject_server_request(request_id.clone(), "user declined input request".into())
                .await;
            let _ = this.update(cx, |view, cx| match result {
                Ok(()) => view.remove_pending_input(&thread_id, &request_id, cx),
                Err(error) => view.apply_thread_error(
                    &thread_id,
                    "user-input",
                    format!("failed to reject input request: {error}"),
                    false,
                    cx,
                ),
            });
        })
        .detach();
    }

    fn sync_active_thread_settings(&mut self, cx: &mut Context<Self>) {
        if self.ui_state.read(cx).new_chat_open {
            return;
        }
        let Some(thread_id) = self.active_chat_entity(cx).and_then(|chat| {
            chat.read(cx)
                .thread
                .as_ref()
                .map(|thread| thread.id.clone())
        }) else {
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
