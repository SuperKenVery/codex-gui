use super::{CodexGui, thread_mapping::*};
use crate::bridge::BridgeEvent;
use crate::gui::{ChatState, HistoryNotice, ProjectState};
use codex_app_server_protocol::{ServerNotification, Thread, ThreadItem};
use gpui::{AppContext, Context, Entity};

impl CodexGui {
    pub(super) fn apply_bridge_event(&mut self, event: BridgeEvent, cx: &mut Context<Self>) {
        match event {
            BridgeEvent::Notification(notification) => {
                self.apply_server_notification(notification, cx)
            }
            BridgeEvent::TransportError(message) => self.apply_bridge_error(message, cx),
            BridgeEvent::Lagged { skipped } => {
                self.apply_bridge_error(
                    format!("embedded app-server event consumer dropped {skipped} events"),
                    cx,
                );
            }
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
            ServerNotification::ThreadDeleted(params) => {
                self.remove_thread_from_ui(&params.thread_id, cx);
            }
            ServerNotification::TurnStarted(params) => {
                tracing::info!(
                    thread_id = %params.thread_id,
                    turn_id = %params.turn.id,
                    "turn running"
                );
                self.upsert_thread_turn(&params.thread_id, params.turn.clone(), cx);
                self.ui_state.update(cx, |state, cx| {
                    state.start_turn(params.thread_id, params.turn.id);
                    cx.notify();
                });
            }
            ServerNotification::ItemStarted(params) => {
                self.start_thread_item(&params.thread_id, &params.turn_id, params.item, cx);
            }
            ServerNotification::AgentMessageDelta(params) => {
                self.append_thread_agent_delta(
                    &params.thread_id,
                    &params.item_id,
                    &params.delta,
                    cx,
                );
            }
            ServerNotification::CommandExecutionOutputDelta(params) => {
                self.append_thread_command_output_delta(
                    &params.thread_id,
                    &params.item_id,
                    &params.delta,
                    cx,
                );
            }
            ServerNotification::FileChangeOutputDelta(params) => {
                self.append_thread_file_change_output_delta(
                    &params.thread_id,
                    &params.item_id,
                    &params.delta,
                    cx,
                );
            }
            ServerNotification::FileChangePatchUpdated(params) => {
                self.update_thread_file_change_item(
                    &params.thread_id,
                    &params.item_id,
                    params.changes.clone(),
                    cx,
                );
            }
            ServerNotification::ItemCompleted(params) => {
                self.complete_thread_item(&params.thread_id, params.item, cx);
            }
            ServerNotification::ThreadStatusChanged(params) => {
                self.update_thread_status(&params.thread_id, params.status.clone(), cx);
                tracing::info!(
                    thread_id = %params.thread_id,
                    status = thread_status_label(&params.status),
                    "thread status changed"
                );
            }
            ServerNotification::TurnCompleted(params) => {
                let turn = params.turn;
                let thread_id = params.thread_id;
                let turn_id = turn.id.clone();
                self.complete_thread_turn(&thread_id, turn, cx);
                self.ui_state.update(cx, |state, cx| {
                    state.finish_turn(&thread_id, &turn_id);
                    cx.notify();
                });
                tracing::info!(thread_id, turn_id, "turn complete");
            }
            ServerNotification::Error(params) => {
                self.apply_bridge_error(params.error.message, cx);
            }
            _ => {}
        }
    }

    pub(super) fn apply_thread_started(&mut self, thread: Thread, cx: &mut Context<Self>) {
        let thread_id = thread.id.clone();
        let updated_at = thread.updated_at;
        let cwd = thread.cwd.to_string_lossy().into_owned();
        let pending_chat = self.pending_thread_chat.take();
        let chat = if let Some(chat) = pending_chat.as_ref() {
            let title = thread_title(thread.name.as_deref(), &thread.preview);
            let subtitle = format!(
                "{} - {}",
                thread_status_label(&thread.status),
                thread.cwd.display()
            );
            chat.update(cx, |chat, cx| {
                chat.adopt_thread(thread, title.into(), subtitle.into());
                cx.notify();
            });
            chat.clone()
        } else {
            chat_entity_from_thread(thread, cx)
        };
        let mut selected_chat_index = 0;
        if let Some(project) = self.ensure_project_for_cwd(&cwd, cx) {
            selected_chat_index = project.update(cx, |project, cx| {
                project.mark_thread_updated_at(updated_at);
                let selected_chat_index = project
                    .chat_index_by_id(&thread_id, cx)
                    .unwrap_or_else(|| project.upsert_chat(chat, &thread_id, cx));
                cx.notify();
                selected_chat_index
            });
        }
        self.state.update(cx, |state, cx| {
            state.sort_projects_by_recent_activity(cx);
            state.select_chat(selected_chat_index);
            cx.notify();
        });
        self.ui_state.update(cx, |state, cx| {
            state.close_new_chat();
            cx.notify();
        });
        tracing::info!(thread_id, "thread ready");
        if let Some((client_user_message_id, text)) = pending_chat
            .as_ref()
            .and_then(|chat| chat.read(cx).pending_user_message_request())
        {
            let settings = self.state.read(cx).chat_settings.clone();
            tracing::info!(thread_id, "starting first turn");
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
                    view.apply_user_submission_result(
                        &thread_id,
                        &client_user_message_id,
                        result,
                        cx,
                    )
                });
            })
            .detach();
        }
    }

    pub(super) fn replace_thread_in_ui(
        &mut self,
        source_thread_id: &str,
        replacement: Thread,
        cx: &mut Context<Self>,
    ) -> bool {
        let replacement_thread_id = replacement.id.clone();
        let updated_at = replacement.updated_at;
        let replacement_chat = chat_entity_from_thread(replacement, cx);

        let projectless_index = self
            .state
            .read(cx)
            .projectless_chats
            .iter()
            .position(|chat| chat.read(cx).id == source_thread_id);
        if let Some(index) = projectless_index {
            self.state.update(cx, |state, cx| {
                state.projectless_chats[index] = replacement_chat;
                state.select_projectless_chat(index);
                cx.notify();
            });
        } else {
            let location = self.state.read(cx).projects.iter().enumerate().find_map(
                |(project_index, project)| {
                    project
                        .read(cx)
                        .chats
                        .iter()
                        .position(|chat| chat.read(cx).id == source_thread_id)
                        .map(|chat_index| (project_index, chat_index, project.clone()))
                },
            );
            let Some((project_index, chat_index, project)) = location else {
                tracing::warn!(
                    source_thread_id,
                    replacement_thread_id,
                    "source thread missing while applying edited replacement"
                );
                return false;
            };
            project.update(cx, |project, cx| {
                project.chats[chat_index] = replacement_chat;
                project.mark_thread_updated_at(updated_at);
                cx.notify();
            });
            self.state.update(cx, |state, cx| {
                state.active_project = project_index;
                state.select_chat(chat_index);
                cx.notify();
            });
        }

        self.ui_state.update(cx, |state, cx| {
            state.close_new_chat();
            cx.notify();
        });
        tracing::info!(
            source_thread_id,
            thread_id = replacement_thread_id,
            "thread replaced in UI"
        );
        true
    }

    pub(super) fn remove_thread_from_ui(&mut self, thread_id: &str, cx: &mut Context<Self>) {
        let active_thread_id = self
            .active_chat_entity(cx)
            .map(|chat| chat.read(cx).id.clone());
        let projects = self.state.read(cx).projects.clone();
        for project in projects {
            project.update(cx, |project, cx| {
                let old_len = project.chats.len();
                project.chats.retain(|chat| chat.read(cx).id != thread_id);
                if project.chats.len() != old_len {
                    cx.notify();
                }
            });
        }

        self.state.update(cx, |state, cx| {
            state
                .projectless_chats
                .retain(|chat| chat.read(cx).id != thread_id);
            if let Some(active_thread_id) = active_thread_id {
                if let Some(index) = state
                    .projectless_chats
                    .iter()
                    .position(|chat| chat.read(cx).id == active_thread_id)
                {
                    state.select_projectless_chat(index);
                } else if let Some((project_index, chat_index)) = state
                    .projects
                    .iter()
                    .enumerate()
                    .find_map(|(project_index, project)| {
                        project
                            .read(cx)
                            .chats
                            .iter()
                            .position(|chat| chat.read(cx).id == active_thread_id)
                            .map(|chat_index| (project_index, chat_index))
                    })
                {
                    state.active_project = project_index;
                    state.select_chat(chat_index);
                } else {
                    state.select_first_chat();
                }
            }
            cx.notify();
        });
        tracing::info!(thread_id, "removed deleted thread from UI");
    }

    pub(super) fn apply_thread_resumed(&mut self, thread: Thread, cx: &mut Context<Self>) {
        let thread_id = thread.id.clone();
        let chat = chat_entity_from_thread(thread, cx);
        if self
            .state
            .read(cx)
            .projectless_chats
            .iter()
            .any(|chat| chat.read(cx).id == thread_id)
        {
            self.state.update(cx, |state, cx| {
                let index = state
                    .projectless_chats
                    .iter()
                    .position(|chat| chat.read(cx).id == thread_id);
                match index {
                    Some(index) => state.projectless_chats[index] = chat,
                    None => state.projectless_chats.insert(0, chat),
                }
                cx.notify();
            });
        } else if let Some(project) = self.active_project_entity(cx) {
            let should_keep_selected = self
                .active_chat_entity(cx)
                .map(|chat| chat.read(cx).id == thread_id)
                .unwrap_or(false);
            let loaded_chat_index = project.update(cx, |project, cx| {
                let loaded_chat_index = project.upsert_chat(chat, &thread_id, cx);
                cx.notify();
                loaded_chat_index
            });
            if should_keep_selected {
                self.state.update(cx, |state, cx| {
                    state.select_chat(loaded_chat_index);
                    cx.notify();
                });
            }
        }
        tracing::info!(thread_id, "thread loaded");
    }

    pub(super) fn apply_bridge_error(&mut self, message: String, cx: &mut Context<Self>) {
        tracing::error!(error = %message, "codex app-server error");
        self.ui_state.update(cx, |state, cx| {
            state.clear_active_turn();
            cx.notify();
        });
        if let Some(chat) = self.active_chat_entity(cx) {
            let thread_id = chat.read(cx).id.clone();
            self.append_notice(&thread_id, message, cx);
        } else if let Some(project) = self.active_project_entity(cx) {
            let chat = cx.new(|_| {
                ChatState::new(
                    "bridge-error".into(),
                    "Bridge error".into(),
                    message.clone().into(),
                    vec![HistoryNotice {
                        id: "bridge-error".into(),
                        body: message.into(),
                    }],
                )
            });
            project.update(cx, |project, cx| {
                project.append_chat(chat);
                cx.notify();
            });
        }
    }

    pub(super) fn active_project_entity(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<Entity<ProjectState>> {
        self.state.read(cx).active_project()
    }

    pub(super) fn active_chat_entity(&self, cx: &mut Context<Self>) -> Option<Entity<ChatState>> {
        self.state.read(cx).active_chat_entity(cx)
    }

    fn ensure_project_for_cwd(
        &self,
        cwd: &str,
        cx: &mut Context<Self>,
    ) -> Option<Entity<ProjectState>> {
        let existing = self
            .state
            .read(cx)
            .projects
            .iter()
            .position(|project| project.read(cx).path.as_ref() == cwd);
        if let Some(index) = existing {
            return self.state.read(cx).projects.get(index).cloned();
        }

        let name = project_name_from_path(cwd);
        let empty_chat = empty_chat_entity(cx);
        let project = cx.new(|_| ProjectState::new(name.into(), cwd.into(), vec![empty_chat]));
        self.state.update(cx, |state, cx| {
            state.projects.push(project.clone());
            state.sort_projects_by_recent_activity(cx);
            let index = state
                .projects
                .iter()
                .position(|candidate| candidate.read(cx).path.as_ref() == cwd)
                .unwrap_or(0);
            state.active_project = index;
            state.active_projectless_chat = None;
            cx.notify();
        });
        Some(project)
    }

    pub(super) fn find_chat_entity(
        &self,
        thread_id: &str,
        cx: &mut Context<Self>,
    ) -> Option<Entity<ChatState>> {
        let state = self.state.read(cx);
        if let Some(chat) = state
            .projectless_chats
            .iter()
            .find(|chat| chat.read(cx).id == thread_id)
        {
            return Some(chat.clone());
        }
        for project in &state.projects {
            let chats = project.read(cx).chats.clone();
            for chat in chats {
                let is_match = chat.read(cx).id == thread_id;
                if is_match {
                    return Some(chat);
                }
            }
        }
        None
    }

    fn append_notice(&self, thread_id: &str, body: String, cx: &mut Context<Self>) {
        let Some(chat) = self.find_chat_entity(thread_id, cx) else {
            return;
        };
        chat.update(cx, |chat, cx| {
            chat.upsert_notice(format!("notice-{thread_id}"), body);
            cx.notify();
        });
    }

    fn update_chat_title(&self, thread_id: &str, title: String, cx: &mut Context<Self>) {
        let Some(chat) = self.find_chat_entity(thread_id, cx) else {
            return;
        };
        chat.update(cx, |chat, cx| {
            chat.set_title(title);
            cx.notify();
        });
    }

    fn upsert_thread_turn(
        &self,
        thread_id: &str,
        turn: codex_app_server_protocol::Turn,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.find_chat_entity(thread_id, cx) else {
            return;
        };
        chat.update(cx, |chat, cx| {
            chat.upsert_turn(turn);
            cx.notify();
        });
    }

    fn complete_thread_turn(
        &self,
        thread_id: &str,
        turn: codex_app_server_protocol::Turn,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.find_chat_entity(thread_id, cx) else {
            return;
        };
        chat.update(cx, |chat, cx| {
            chat.complete_turn(turn);
            cx.notify();
        });
    }

    fn start_thread_item(
        &self,
        thread_id: &str,
        turn_id: &str,
        item: ThreadItem,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.find_chat_entity(thread_id, cx) else {
            return;
        };
        chat.update(cx, |chat, cx| {
            chat.start_item(turn_id, item);
            cx.notify();
        });
    }

    fn complete_thread_item(&self, thread_id: &str, item: ThreadItem, cx: &mut Context<Self>) {
        let Some(chat) = self.find_chat_entity(thread_id, cx) else {
            return;
        };
        chat.update(cx, |chat, cx| {
            chat.complete_item(item);
            cx.notify();
        });
    }

    fn append_thread_agent_delta(
        &self,
        thread_id: &str,
        item_id: &str,
        delta: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.find_chat_entity(thread_id, cx) else {
            return;
        };
        chat.update(cx, |chat, cx| {
            chat.append_agent_text_delta(item_id, delta);
            cx.notify();
        });
    }

    fn append_thread_command_output_delta(
        &self,
        thread_id: &str,
        item_id: &str,
        delta: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.find_chat_entity(thread_id, cx) else {
            return;
        };
        chat.update(cx, |chat, cx| {
            chat.append_command_output_delta(item_id, delta);
            cx.notify();
        });
    }

    fn append_thread_file_change_output_delta(
        &self,
        thread_id: &str,
        item_id: &str,
        delta: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.find_chat_entity(thread_id, cx) else {
            return;
        };
        chat.update(cx, |chat, cx| {
            chat.append_file_change_output_delta(item_id, delta);
            cx.notify();
        });
    }

    fn update_thread_file_change_item(
        &self,
        thread_id: &str,
        item_id: &str,
        changes: Vec<codex_app_server_protocol::FileUpdateChange>,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.find_chat_entity(thread_id, cx) else {
            return;
        };
        chat.update(cx, |chat, cx| {
            chat.update_file_change_item(item_id, changes);
            cx.notify();
        });
    }

    fn update_thread_status(
        &self,
        thread_id: &str,
        status: codex_app_server_protocol::ThreadStatus,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.find_chat_entity(thread_id, cx) else {
            return;
        };
        chat.update(cx, |chat, cx| {
            chat.set_thread_status(status);
            cx.notify();
        });
    }
}
