use std::time::Instant;

use gpui::{Entity, SharedString};

pub struct GuiState {
    pub projects: Vec<Entity<ProjectState>>,
    pub active_project: usize,
    pub active_chat: usize,
    pub chat_settings: ChatSettings,
    pub available_models: Vec<ModelOption>,
    pub permission_profiles: Vec<PermissionProfileOption>,
}

impl GuiState {
    pub fn new(projects: Vec<Entity<ProjectState>>) -> Self {
        Self {
            projects,
            active_project: 0,
            active_chat: 0,
            chat_settings: ChatSettings::default(),
            available_models: Vec::new(),
            permission_profiles: default_permission_profiles(),
        }
    }

    pub fn active_project(&self) -> Option<Entity<ProjectState>> {
        self.projects.get(self.active_project).cloned()
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
}

impl ProjectState {
    pub fn new(name: SharedString, path: SharedString, chats: Vec<Entity<ChatState>>) -> Self {
        Self { name, path, chats }
    }
}

pub struct ChatState {
    pub id: String,
    pub title: SharedString,
    pub subtitle: SharedString,
    pub messages: Vec<Entity<MessageState>>,
}

impl ChatState {
    pub fn new(
        id: String,
        title: SharedString,
        subtitle: SharedString,
        messages: Vec<Entity<MessageState>>,
    ) -> Self {
        Self {
            id,
            title,
            subtitle,
            messages,
        }
    }
}

pub struct MessageState {
    pub message: Message,
    pub created_at: Instant,
    pub updated_at: Instant,
    pub tools_expanded: bool,
}

impl MessageState {
    pub fn new(message: Message) -> Self {
        let now = Instant::now();
        Self {
            message,
            created_at: now,
            updated_at: now,
            tools_expanded: false,
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = Instant::now();
    }

    pub fn toggle_tools(&mut self) {
        self.tools_expanded = !self.tools_expanded;
        self.touch();
    }
}

#[derive(Clone)]
pub enum Message {
    User(String),
    Assistant {
        id: String,
        body: String,
        state: StreamState,
        tools: Vec<ToolCall>,
    },
    Commentary(String),
}

#[derive(Clone, Copy)]
pub enum StreamState {
    Complete,
    Streaming,
}

#[derive(Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub status: ToolStatus,
    pub detail: String,
}

#[derive(Clone, Copy)]
pub enum ToolStatus {
    Running,
    Done,
}

pub struct BridgeState {
    pub status: String,
}

impl BridgeState {
    pub fn new() -> Self {
        Self {
            status: "starting codex app-server".into(),
        }
    }
}

pub struct UiState {
    pub side_chat_open: bool,
}

impl UiState {
    pub fn new() -> Self {
        Self {
            side_chat_open: false,
        }
    }
}
