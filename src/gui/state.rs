use std::collections::HashMap;

use codex_app_server_protocol::{
    FileUpdateChange, Thread, ThreadItem, ThreadStatus, Turn, TurnStatus,
};
use gpui::{AppContext, Context, Entity, SharedString};

pub(crate) fn single_line_title(title: &str) -> String {
    title
        .split(['\r', '\n'])
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

pub struct GuiState {
    pub projects: Vec<Entity<ProjectState>>,
    pub projectless_chats: Vec<Entity<ChatState>>,
    pub active_project: usize,
    pub active_chat: usize,
    pub active_projectless_chat: Option<usize>,
    /// Whether the app server has answered the initial thread listing.
    pub threads_loaded: bool,
    pub chat_settings: ChatSettings,
    pub available_models: Vec<ModelOption>,
    pub permission_profiles: Vec<PermissionProfileOption>,
}

impl GuiState {
    pub fn new() -> Self {
        Self {
            projects: Vec::new(),
            projectless_chats: Vec::new(),
            active_project: 0,
            active_chat: 0,
            active_projectless_chat: None,
            threads_loaded: false,
            chat_settings: ChatSettings::default(),
            available_models: Vec::new(),
            permission_profiles: default_permission_profiles(),
        }
    }

    /// The currently selected chat, whether it lives under a project or in the
    /// project-less chat list.
    pub fn active_chat_entity(&self, cx: &impl AppContext) -> Option<Entity<ChatState>> {
        if let Some(index) = self.active_projectless_chat {
            return self.projectless_chats.get(index).cloned();
        }
        let project = self.projects.get(self.active_project)?;
        let chats = project.read_with(cx, |project, _| project.chats.clone());
        chats.get(self.active_chat).cloned()
    }

    pub fn active_project(&self) -> Option<Entity<ProjectState>> {
        self.projects.get(self.active_project).cloned()
    }

    pub fn project_index_by_path(&self, path: &str, cx: &mut Context<Self>) -> Option<usize> {
        self.projects
            .iter()
            .position(|project| project.read(cx).path.as_ref() == path)
    }

    pub fn select_project(&mut self, index: usize) {
        self.active_project = index;
        self.active_chat = 0;
        self.active_projectless_chat = None;
    }

    pub fn sort_projects_by_recent_activity(&mut self, cx: &mut Context<Self>) {
        let active_path = self
            .active_project()
            .map(|project| project.read(cx).path.to_string());
        let mut projects = self
            .projects
            .iter()
            .enumerate()
            .map(|(index, project)| {
                let project_state = project.read(cx);
                (
                    index,
                    project_state.latest_thread_updated_at,
                    project_state.path.to_string(),
                    project.clone(),
                )
            })
            .collect::<Vec<_>>();

        projects.sort_by(|(a_index, a_updated_at, ..), (b_index, b_updated_at, ..)| {
            b_updated_at
                .cmp(a_updated_at)
                .then_with(|| a_index.cmp(b_index))
        });

        self.projects = projects
            .into_iter()
            .map(|(_, _, _, project)| project)
            .collect();

        if let Some(active_path) = active_path
            && let Some(index) = self
                .projects
                .iter()
                .position(|project| project.read(cx).path.as_ref() == active_path.as_str())
        {
            self.active_project = index;
        } else {
            self.active_project = self
                .active_project
                .min(self.projects.len().saturating_sub(1));
        }
    }

    pub fn select_chat(&mut self, index: usize) {
        self.active_chat = index;
        self.active_projectless_chat = None;
    }

    pub fn select_projectless_chat(&mut self, index: usize) {
        self.active_projectless_chat = Some(index);
        self.active_chat = 0;
    }

    pub fn add_project(&mut self, project: Entity<ProjectState>) -> usize {
        self.projects.push(project);
        self.active_project = self.projects.len() - 1;
        self.active_chat = 0;
        self.active_projectless_chat = None;
        self.active_project
    }

    pub fn select_first_chat(&mut self) {
        self.active_chat = 0;
    }

    pub fn set_model(&mut self, model: String) {
        self.chat_settings.model = model;
        if let Some(option) = self
            .available_models
            .iter()
            .find(|option| option.id == self.chat_settings.model)
        {
            self.chat_settings.effort = option.default_effort.clone();
        }
    }

    pub fn set_effort(&mut self, effort: String) {
        self.chat_settings.effort = effort;
    }

    pub fn set_permission_profile(&mut self, permission_profile: String) {
        self.chat_settings.permission_profile = permission_profile;
    }

    pub fn set_approvals_reviewer(&mut self, approvals_reviewer: ApprovalReviewerMode) {
        self.chat_settings.approvals_reviewer = approvals_reviewer;
    }

    pub fn set_available_models(&mut self, models: Vec<ModelOption>) {
        if let Some(default_model) = models
            .first()
            .filter(|_| self.chat_settings.model.is_empty())
        {
            self.chat_settings.model = default_model.id.clone();
            self.chat_settings.effort = default_model.default_effort.clone();
        }
        self.available_models = models;
    }

    pub fn set_permission_profiles(&mut self, profiles: Vec<PermissionProfileOption>) {
        self.permission_profiles = profiles;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatSettings {
    pub model: String,
    pub effort: String,
    pub permission_profile: String,
    pub approvals_reviewer: ApprovalReviewerMode,
}

impl Default for ChatSettings {
    fn default() -> Self {
        Self {
            model: "gpt-5.5".into(),
            effort: "medium".into(),
            permission_profile: PermissionMode::WorkspaceWrite.profile_id().into(),
            approvals_reviewer: ApprovalReviewerMode::User,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelOption {
    pub id: String,
    pub display_name: String,
    pub supported_efforts: Vec<String>,
    pub default_effort: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionProfileOption {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionMode {
    ReadOnly,
    WorkspaceWrite,
    DangerFullAccess,
}

impl PermissionMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::ReadOnly => "Read only",
            Self::WorkspaceWrite => "Workspace write",
            Self::DangerFullAccess => "Full access",
        }
    }

    pub fn profile_id(self) -> &'static str {
        match self {
            Self::ReadOnly => ":read-only",
            Self::WorkspaceWrite => ":workspace",
            Self::DangerFullAccess => ":danger-full-access",
        }
    }
}

pub fn permission_profile_label(id: &str) -> String {
    match id {
        ":read-only" => PermissionMode::ReadOnly.label().into(),
        ":workspace" => PermissionMode::WorkspaceWrite.label().into(),
        ":danger-full-access" => PermissionMode::DangerFullAccess.label().into(),
        other => other.trim_start_matches(':').to_string(),
    }
}

fn default_permission_profiles() -> Vec<PermissionProfileOption> {
    [
        PermissionMode::ReadOnly,
        PermissionMode::WorkspaceWrite,
        PermissionMode::DangerFullAccess,
    ]
    .into_iter()
    .map(|mode| PermissionProfileOption {
        id: mode.profile_id().into(),
        label: mode.label().into(),
        description: None,
    })
    .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalReviewerMode {
    User,
    AutoReview,
}

impl ApprovalReviewerMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::User => "Ask me",
            Self::AutoReview => "Approve for me",
        }
    }
}

pub struct ProjectState {
    pub name: SharedString,
    pub path: SharedString,
    pub chats: Vec<Entity<ChatState>>,
    pub latest_thread_updated_at: Option<i64>,
}

impl ProjectState {
    pub fn new(name: SharedString, path: SharedString, chats: Vec<Entity<ChatState>>) -> Self {
        Self {
            name,
            path,
            chats,
            latest_thread_updated_at: None,
        }
    }

    pub fn replace_loaded_chats(
        &mut self,
        chats: Vec<Entity<ChatState>>,
        latest_thread_updated_at: Option<i64>,
    ) {
        self.chats = chats;
        self.latest_thread_updated_at = latest_thread_updated_at;
    }

    pub fn mark_thread_updated_at(&mut self, updated_at: i64) {
        self.latest_thread_updated_at = Some(
            self.latest_thread_updated_at
                .map(|current| current.max(updated_at))
                .unwrap_or(updated_at),
        );
    }

    pub fn chat_index_by_id(&self, chat_id: &str, cx: &mut Context<Self>) -> Option<usize> {
        self.chats
            .iter()
            .position(|chat| chat.read(cx).id == chat_id)
    }

    pub fn upsert_chat(
        &mut self,
        chat: Entity<ChatState>,
        chat_id: &str,
        cx: &mut Context<Self>,
    ) -> usize {
        if let Some(index) = self.chat_index_by_id(chat_id, cx) {
            self.chats[index] = chat;
            index
        } else {
            self.chats.insert(0, chat);
            0
        }
    }

    pub fn append_chat(&mut self, chat: Entity<ChatState>) {
        self.chats.push(chat);
    }
}

pub struct ChatState {
    pub id: String,
    pub title: SharedString,
    pub subtitle: SharedString,
    pub thread: Option<Thread>,
    pub notices: Vec<HistoryNotice>,
    message_states: HashMap<String, MessageState>,
    item_locations: HashMap<String, ThreadItemLocation>,
}

impl ChatState {
    pub fn new(
        id: String,
        title: SharedString,
        subtitle: SharedString,
        notices: Vec<HistoryNotice>,
    ) -> Self {
        Self {
            id,
            title,
            subtitle,
            thread: None,
            notices,
            message_states: HashMap::new(),
            item_locations: HashMap::new(),
        }
    }

    pub fn from_thread(thread: Thread, title: SharedString, subtitle: SharedString) -> Self {
        let id = thread.id.clone();
        let item_locations = thread_item_locations(&thread);
        Self {
            id,
            title,
            subtitle,
            thread: Some(thread),
            notices: Vec::new(),
            message_states: HashMap::new(),
            item_locations,
        }
    }

    pub fn upsert_notice(&mut self, id: String, body: String) {
        if let Some(notice) = self.notices.iter_mut().find(|notice| notice.id == id) {
            notice.body = body.into();
        } else {
            self.notices.push(HistoryNotice {
                id,
                body: body.into(),
            });
        }
    }

    pub fn set_title(&mut self, title: String) {
        if let Some(thread) = &mut self.thread {
            thread.name = Some(title.clone());
        }
        self.title = single_line_title(&title).into();
    }

    pub fn set_thread_status(&mut self, status: ThreadStatus) {
        if let Some(thread) = &mut self.thread {
            thread.status = status;
        }
    }

    pub fn upsert_turn(&mut self, turn: Turn) {
        if !matches!(turn.status, TurnStatus::InProgress) {
            for item in &turn.items {
                self.message_states.remove(item.id());
            }
        }
        let Some(thread) = &mut self.thread else {
            return;
        };
        if let Some(existing) = thread
            .turns
            .iter_mut()
            .find(|existing| existing.id == turn.id)
        {
            *existing = turn;
        } else {
            thread.turns.push(turn);
        }
        self.rebuild_item_locations();
    }

    pub fn start_item(&mut self, turn_id: &str, item: ThreadItem) {
        let item_id = item.id().to_string();
        self.message_states
            .insert(item_id, MessageState::streaming());
        self.upsert_thread_item(turn_id, item);
    }

    pub fn complete_item(&mut self, item: ThreadItem) {
        let item_id = item.id().to_string();
        self.replace_thread_item(item);
        if let Some(state) = self.message_states.get_mut(&item_id) {
            state.lifecycle = MessageLifecycle::Complete;
        }
    }

    fn upsert_thread_item(&mut self, turn_id: &str, item: ThreadItem) {
        let Some(thread) = &mut self.thread else {
            return;
        };
        let item_id = item.id().to_string();
        let mut changed = false;
        if let Some(turn) = thread.turns.iter_mut().find(|turn| turn.id == turn_id) {
            if let Some(existing) = turn
                .items
                .iter_mut()
                .find(|existing| existing.id() == item_id)
            {
                *existing = item;
                changed = true;
            } else {
                turn.items.push(item);
                changed = true;
            }
        } else if let Some(turn) = thread.turns.last_mut() {
            if let Some(existing) = turn
                .items
                .iter_mut()
                .find(|existing| existing.id() == item_id)
            {
                *existing = item;
                changed = true;
            } else {
                turn.items.push(item);
                changed = true;
            }
        }
        if changed {
            self.rebuild_item_locations();
        }
    }

    pub fn replace_thread_item(&mut self, item: ThreadItem) {
        let Some(thread) = &mut self.thread else {
            return;
        };
        let item_id = item.id().to_string();
        for turn in &mut thread.turns {
            if let Some(existing) = turn
                .items
                .iter_mut()
                .find(|existing| existing.id() == item_id)
            {
                *existing = item;
                self.rebuild_item_locations();
                return;
            }
        }
    }

    pub fn append_agent_text_delta(&mut self, item_id: &str, delta: &str) {
        let Some(ThreadItem::AgentMessage { text, .. }) = self.thread_item_mut(item_id) else {
            return;
        };
        text.push_str(delta);
    }

    pub fn append_command_output_delta(&mut self, item_id: &str, delta: &str) {
        let Some(ThreadItem::CommandExecution {
            aggregated_output, ..
        }) = self.thread_item_mut(item_id)
        else {
            return;
        };
        aggregated_output
            .get_or_insert_with(String::new)
            .push_str(delta);
    }

    pub fn update_file_change_item(&mut self, item_id: &str, changes: Vec<FileUpdateChange>) {
        let Some(ThreadItem::FileChange {
            changes: existing, ..
        }) = self.thread_item_mut(item_id)
        else {
            return;
        };
        *existing = changes;
    }

    pub fn append_file_change_output_delta(&mut self, item_id: &str, delta: &str) {
        let Some(ThreadItem::FileChange { changes, .. }) = self.thread_item_mut(item_id) else {
            return;
        };
        if let Some(last_change) = changes.last_mut() {
            last_change.diff.push_str(delta);
        }
    }

    pub fn item_is_streaming(&self, item_id: &str) -> bool {
        self.message_states
            .get(item_id)
            .is_some_and(MessageState::is_streaming)
    }

    fn thread_item_mut(&mut self, item_id: &str) -> Option<&mut ThreadItem> {
        let location = *self.item_locations.get(item_id)?;
        self.thread
            .as_mut()?
            .turns
            .get_mut(location.turn_index)?
            .items
            .get_mut(location.item_index)
    }

    fn rebuild_item_locations(&mut self) {
        self.item_locations = self
            .thread
            .as_ref()
            .map(thread_item_locations)
            .unwrap_or_default();
    }
}

#[derive(Clone)]
pub struct HistoryNotice {
    pub id: String,
    pub body: SharedString,
}

/// Runtime lifecycle supplied by item notifications but absent from `ThreadItem`.
pub struct MessageState {
    lifecycle: MessageLifecycle,
}

enum MessageLifecycle {
    Streaming,
    Complete,
}

impl MessageState {
    fn streaming() -> Self {
        Self {
            lifecycle: MessageLifecycle::Streaming,
        }
    }

    fn is_streaming(&self) -> bool {
        matches!(self.lifecycle, MessageLifecycle::Streaming)
    }
}

#[derive(Clone, Copy)]
struct ThreadItemLocation {
    turn_index: usize,
    item_index: usize,
}

fn thread_item_locations(thread: &Thread) -> HashMap<String, ThreadItemLocation> {
    let mut locations = HashMap::new();
    for (turn_index, turn) in thread.turns.iter().enumerate() {
        for (item_index, item) in turn.items.iter().enumerate() {
            locations.insert(
                item.id().to_string(),
                ThreadItemLocation {
                    turn_index,
                    item_index,
                },
            );
        }
    }
    locations
}

pub struct UiState {
    pub side_chat_open: bool,
    pub new_chat_open: bool,
    pub active_turn: Option<ActiveTurn>,
}

impl UiState {
    pub fn new() -> Self {
        Self {
            side_chat_open: false,
            new_chat_open: true,
            active_turn: None,
        }
    }

    pub fn open_new_chat(&mut self) {
        self.new_chat_open = true;
    }

    pub fn close_new_chat(&mut self) {
        self.new_chat_open = false;
    }

    pub fn toggle_side_chat(&mut self) {
        self.side_chat_open = !self.side_chat_open;
    }

    pub fn start_turn(&mut self, thread_id: String, turn_id: String) {
        self.active_turn = Some(ActiveTurn { thread_id, turn_id });
    }

    pub fn finish_turn(&mut self, thread_id: &str, turn_id: &str) {
        if self.active_turn.as_ref().is_some_and(|active_turn| {
            active_turn.thread_id == thread_id && active_turn.turn_id == turn_id
        }) {
            self.active_turn = None;
        }
    }

    pub fn clear_active_turn(&mut self) {
        self.active_turn = None;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveTurn {
    pub thread_id: String,
    pub turn_id: String,
}
