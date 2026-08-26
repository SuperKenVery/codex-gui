//! Read RPC responses and update view state accordingly

use super::{
    CodexGui,
    thread_mapping::{chat_entity_from_thread, empty_chat_entity},
};
use crate::{
    bridge::BridgeError,
    global_state::{CodexGlobalState, LastSelectedModel},
    gui::{ChatState, ModelOption, PermissionProfileOption, ProjectState},
};
use codex_app_server_protocol::Thread;
use gpui::{AppContext, Context, Entity};
use std::collections::{HashMap, HashSet};

impl CodexGui {
    pub(super) fn apply_initialize_result(
        &mut self,
        result: Result<(), BridgeError>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(()) => {
                tracing::info!("started embedded codex app-server");
                self.load_startup_data(cx);
            }
            Err(err) => self.apply_bridge_error(err.to_string(), cx),
        }
    }

    pub(super) fn apply_threads_result(
        &mut self,
        result: Result<Vec<Thread>, BridgeError>,
        cx: &mut Context<Self>,
    ) {
        let threads = match result {
            Ok(result) => result,
            Err(err) => {
                self.apply_bridge_error(err.to_string(), cx);
                self.state.update(cx, |state, cx| {
                    state.threads_loaded = true;
                    cx.notify();
                });
                return;
            }
        };

        let thread_count = threads.len();
        let projectless_ids = projectless_thread_ids();
        let mut project_paths = Vec::new();
        let mut threads_by_project = HashMap::<String, Vec<Thread>>::new();
        let mut projectless_threads = Vec::new();
        let mut default_thread: Option<(i64, String)> = None;
        for thread in threads {
            let is_newest = default_thread
                .as_ref()
                .is_none_or(|(updated_at, _)| thread.updated_at > *updated_at);
            if is_newest {
                default_thread = Some((thread.updated_at, thread.id.clone()));
            }
            if projectless_ids.contains(&thread.id) {
                projectless_threads.push(thread);
                continue;
            }
            let cwd = thread.cwd.to_string_lossy().into_owned();
            if !threads_by_project.contains_key(&cwd) {
                project_paths.push(cwd.clone());
            }
            threads_by_project.entry(cwd).or_default().push(thread);
        }

        let existing_projects = self.state.read(cx).projects.clone();
        let existing_paths = existing_projects
            .iter()
            .map(|project| project.read(cx).path.to_string())
            .collect::<HashSet<_>>();

        for project in existing_projects {
            let cwd = project.read(cx).path.to_string();
            let (chats, latest_thread_updated_at) =
                loaded_chats(threads_by_project.remove(&cwd).unwrap_or_default(), cx);
            project.update(cx, |project, cx| {
                project.replace_loaded_chats(chats, latest_thread_updated_at);
                cx.notify();
            });
        }

        let mut discovered_projects = Vec::new();
        for cwd in project_paths {
            if existing_paths.contains(&cwd) {
                continue;
            }
            let Some(threads) = threads_by_project.remove(&cwd) else {
                continue;
            };
            let (chats, latest_thread_updated_at) = loaded_chats(threads, cx);
            let name = super::thread_mapping::project_name_from_path(&cwd);
            discovered_projects.push(cx.new(|_| {
                let mut project = ProjectState::new(name.into(), cwd.into(), chats);
                project.latest_thread_updated_at = latest_thread_updated_at;
                project
            }));
        }

        let projectless_chats = if projectless_threads.is_empty() {
            Vec::new()
        } else {
            loaded_chats(projectless_threads, cx).0
        };

        self.state.update(cx, |state, cx| {
            state.projects.extend(discovered_projects);
            state.projectless_chats = projectless_chats;
            state.sort_projects_by_recent_activity(cx);
            state.select_first_chat();
            state.threads_loaded = true;
            cx.notify();
        });

        let project_count = self.state.read(cx).projects.len();
        let projectless_count = self.state.read(cx).projectless_chats.len();
        tracing::info!(
            thread_count,
            project_count,
            projectless_count,
            "loaded threads from app server"
        );
        let can_resume_default = !self.ui_state.read(cx).new_chat_open;
        if can_resume_default && let Some((_, thread_id)) = default_thread {
            if projectless_ids.contains(&thread_id) {
                self.state.update(cx, |state, cx| {
                    state.active_projectless_chat = state
                        .projectless_chats
                        .iter()
                        .position(|chat| chat.read(cx).id == thread_id);
                    cx.notify();
                });
            }
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

    pub(super) fn apply_models_result(
        &mut self,
        result: Result<Vec<ModelOption>, BridgeError>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(models) => {
                let last_selected_model = last_selected_model();
                self.state.update(cx, |state, cx| {
                    state.set_available_models(models);
                    if let Some(selection) = last_selected_model
                        && let Some(model) =
                            resolve_model_id(&selection.slug, &state.available_models)
                    {
                        state.set_model(model.clone());
                        if let Some(thinking_effort) = selection.thinking_effort
                            && let Some(effort) = resolve_reasoning_effort(
                                &thinking_effort,
                                &model,
                                &state.available_models,
                            )
                        {
                            state.set_effort(effort);
                        }
                    }
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
            Err(err) => {
                let message = err.to_string();
                if let Some(pending) = self.pending_thread.take() {
                    pending.chat.update(cx, |chat, cx| {
                        if let Some((client_id, _)) = chat.pending_user_message_request() {
                            chat.fail_user_message(&client_id, message.clone());
                        }
                        chat.upsert_notice("thread-start-error".into(), message.clone());
                        cx.notify();
                    });
                    tracing::error!(error = %message, "failed to start thread");
                } else {
                    self.apply_bridge_error(message, cx);
                }
            }
        }
    }

    pub(super) fn apply_user_submission_result(
        &mut self,
        thread_id: &str,
        client_user_message_id: &str,
        result: Result<(), BridgeError>,
        cx: &mut Context<Self>,
    ) {
        let Err(err) = result else {
            return;
        };
        let message = err.to_string();
        tracing::error!(
            thread_id,
            client_user_message_id,
            error = %message,
            "user message submission failed"
        );
        self.ui_state.update(cx, |state, cx| {
            if state
                .active_turn
                .as_ref()
                .is_some_and(|turn| turn.thread_id == thread_id)
            {
                state.clear_active_turn();
                cx.notify();
            }
        });
        if let Some(chat) = self.find_chat_entity(thread_id, cx) {
            chat.update(cx, |chat, cx| {
                chat.fail_user_message(client_user_message_id, message.clone());
                chat.upsert_notice(format!("send-error-{client_user_message_id}"), message);
                cx.notify();
            });
        }
    }

    pub(super) fn apply_thread_resumed_result(
        &mut self,
        requested_thread_id: &str,
        result: Result<Thread, BridgeError>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(thread) => self.apply_thread_resumed(thread, cx),
            Err(err) => self.apply_bridge_error(err.to_string(), cx),
        }
        self.ui_state.update(cx, |state, cx| {
            state.finish_thread_load(requested_thread_id);
            cx.notify();
        });
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

fn loaded_chats(
    threads: Vec<Thread>,
    cx: &mut Context<CodexGui>,
) -> (Vec<Entity<ChatState>>, Option<i64>) {
    let latest_thread_updated_at = threads.iter().map(|thread| thread.updated_at).max();
    let chats = if threads.is_empty() {
        vec![empty_chat_entity(cx)]
    } else {
        threads
            .into_iter()
            .map(|thread| chat_entity_from_thread(thread, cx))
            .collect()
    };
    (chats, latest_thread_updated_at)
}

/// Thread IDs that the Codex desktop marks as project-less, so they are shown
/// outside of any project group in the sidebar.
fn projectless_thread_ids() -> HashSet<String> {
    CodexGlobalState::load()
        .map(|state| state.projectless_thread_ids())
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "failed to load project-less thread ids");
            HashSet::new()
        })
}

fn last_selected_model() -> Option<LastSelectedModel> {
    CodexGlobalState::load()
        .map(|state| state.last_selected_model())
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "failed to load last selected model");
            None
        })
}

fn resolve_model_id(slug: &str, models: &[ModelOption]) -> Option<String> {
    if models.iter().any(|model| model.id == slug) {
        return Some(slug.to_owned());
    }

    let family = slug
        .strip_suffix("-thinking")
        .or_else(|| slug.strip_suffix("-pro"))
        .unwrap_or(slug);
    let version = family.strip_prefix("gpt-").and_then(|version| {
        let (major, minor) = version.split_once('-')?;
        (!major.is_empty() && !minor.is_empty() && !minor.contains('-'))
            .then(|| format!("gpt-{major}.{minor}"))
    })?;

    [version.clone(), format!("{version}-sol")]
        .into_iter()
        .find(|candidate| models.iter().any(|model| model.id == *candidate))
}

fn resolve_reasoning_effort(
    thinking_effort: &str,
    model_id: &str,
    models: &[ModelOption],
) -> Option<String> {
    let effort = match thinking_effort {
        "standard" => "medium",
        "extended" => "high",
        effort => effort,
    };
    models
        .iter()
        .find(|model| model.id == model_id)?
        .supported_efforts
        .iter()
        .any(|supported| supported == effort)
        .then(|| effort.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_desktop_model_and_effort_to_available_codex_options() {
        let models = vec![ModelOption {
            id: "gpt-5.6-sol".to_string(),
            display_name: "GPT-5.6-Sol".to_string(),
            supported_efforts: vec![
                "medium".to_string(),
                "high".to_string(),
                "xhigh".to_string(),
            ],
            default_effort: "medium".to_string(),
        }];

        let model = resolve_model_id("gpt-5-6-thinking", &models).unwrap();
        assert_eq!(model, "gpt-5.6-sol");
        assert_eq!(
            resolve_reasoning_effort("extended", &model, &models).as_deref(),
            Some("high")
        );
    }
}
