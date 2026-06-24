use std::time::Instant;

use gpui::{AppContext, Context, Entity, SharedString};
use gpui_component::text::TextViewState;
use zed_markdown::Markdown as ZedMarkdown;

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

    pub fn project_index_by_path(&self, path: &str, cx: &mut Context<Self>) -> Option<usize> {
        self.projects
            .iter()
            .position(|project| project.read(cx).path.as_ref() == path)
    }

    pub fn select_project(&mut self, index: usize) {
        self.active_project = index;
        self.active_chat = 0;
    }

    pub fn select_chat(&mut self, index: usize) {
        self.active_chat = index;
    }

    pub fn add_project(&mut self, project: Entity<ProjectState>) -> usize {
        self.projects.push(project);
        self.active_project = self.projects.len() - 1;
        self.active_chat = 0;
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
    pub threads_loaded: bool,
}

impl ProjectState {
    pub fn new(name: SharedString, path: SharedString, chats: Vec<Entity<ChatState>>) -> Self {
        Self {
            name,
            path,
            chats,
            threads_loaded: false,
        }
    }

    pub fn replace_loaded_chats(&mut self, chats: Vec<Entity<ChatState>>) {
        self.chats = chats;
        self.threads_loaded = true;
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

    pub fn append_message(&mut self, message: Entity<MessageState>) {
        self.messages.push(message);
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title.into();
    }
}

pub struct MessageState {
    pub message: Message,
    pub created_at: Instant,
    pub updated_at: Instant,
    pub tools_expanded: bool,
    pub body_view: Option<Entity<TextViewState>>,
    pub zed_markdown: Option<Entity<ZedMarkdown>>,
    pub collapse_tools: bool,
    pub hide_tools: bool,
    pub active_tool_tail: bool,
}

impl MessageState {
    pub fn new(message: Message, cx: &mut Context<Self>) -> Self {
        let now = Instant::now();
        let body_view =
            markdown_body(&message).map(|body| cx.new(|cx| TextViewState::markdown(body, cx)));
        let zed_markdown = markdown_body(&message)
            .map(|body| cx.new(|cx| ZedMarkdown::new(body.into(), None, None, cx)));
        Self {
            message,
            created_at: now,
            updated_at: now,
            tools_expanded: false,
            body_view,
            zed_markdown,
            collapse_tools: true,
            hide_tools: false,
            active_tool_tail: false,
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = Instant::now();
    }

    pub fn toggle_tools(&mut self) {
        self.tools_expanded = !self.tools_expanded;
        self.touch();
    }

    pub fn sync_body_view(&mut self, cx: &mut Context<Self>) {
        match markdown_body(&self.message) {
            Some(body) => {
                if let Some(body_view) = &self.body_view {
                    body_view.update(cx, |body_view, cx| body_view.set_text(body, cx));
                } else {
                    self.body_view = Some(cx.new(|cx| TextViewState::markdown(body, cx)));
                }
                if let Some(zed_markdown) = &self.zed_markdown {
                    zed_markdown.update(cx, |markdown, cx| markdown.replace(body.to_string(), cx));
                } else {
                    self.zed_markdown =
                        Some(cx.new(|cx| ZedMarkdown::new(body.into(), None, None, cx)));
                }
            }
            None => {
                self.body_view = None;
                self.zed_markdown = None;
            }
        }
    }

    pub fn append_body_view_delta(&mut self, delta: &str, cx: &mut Context<Self>) {
        if delta.is_empty() {
            return;
        }

        match markdown_body(&self.message) {
            Some(body) => {
                if let Some(body_view) = &self.body_view {
                    body_view.update(cx, |body_view, cx| body_view.push_str(delta, cx));
                } else {
                    self.body_view = Some(cx.new(|cx| TextViewState::markdown(body, cx)));
                }
                if let Some(zed_markdown) = &self.zed_markdown {
                    zed_markdown.update(cx, |markdown, cx| markdown.append(delta, cx));
                } else {
                    self.zed_markdown =
                        Some(cx.new(|cx| ZedMarkdown::new(body.into(), None, None, cx)));
                }
            }
            None => {
                self.body_view = None;
                self.zed_markdown = None;
            }
        }
    }

    pub fn set_render_options(
        &mut self,
        collapse_tools: bool,
        hide_tools: bool,
        active_tool_tail: bool,
    ) -> bool {
        let changed = self.collapse_tools != collapse_tools
            || self.hide_tools != hide_tools
            || self.active_tool_tail != active_tool_tail;
        self.collapse_tools = collapse_tools;
        self.hide_tools = hide_tools;
        self.active_tool_tail = active_tool_tail;
        changed
    }
}

fn markdown_body(message: &Message) -> Option<&str> {
    match message {
        Message::Notice(body) => Some(body),
        Message::Assistant { body, .. } => Some(body),
        Message::User(_) => None,
    }
}

#[derive(Clone)]
pub enum Message {
    User(String),
    Assistant {
        id: String,
        body: String,
        state: StreamState,
        phase: AssistantPhase,
        tools: Vec<ToolCall>,
    },
    Notice(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AssistantPhase {
    Commentary,
    FinalAnswer,
}

#[derive(Clone, Copy)]
pub enum StreamState {
    Complete,
    Streaming,
}

#[derive(Clone)]
pub enum ToolCall {
    Command {
        id: String,
        command: String,
        cwd: String,
        status: ToolStatus,
    },
    FileChange {
        id: String,
        changes: Vec<FileChangeSummary>,
        status: ToolStatus,
    },
    Mcp {
        id: String,
        server: String,
        tool: String,
        status: ToolStatus,
    },
    Dynamic {
        id: String,
        namespace: Option<String>,
        tool: String,
        status: ToolStatus,
    },
}

impl ToolCall {
    pub fn id(&self) -> &str {
        match self {
            ToolCall::Command { id, .. }
            | ToolCall::FileChange { id, .. }
            | ToolCall::Mcp { id, .. }
            | ToolCall::Dynamic { id, .. } => id,
        }
    }

    pub fn status(&self) -> ToolStatus {
        match self {
            ToolCall::Command { status, .. }
            | ToolCall::FileChange { status, .. }
            | ToolCall::Mcp { status, .. }
            | ToolCall::Dynamic { status, .. } => *status,
        }
    }

    pub fn set_status(&mut self, next_status: ToolStatus) {
        match self {
            ToolCall::Command { status, .. }
            | ToolCall::FileChange { status, .. }
            | ToolCall::Mcp { status, .. }
            | ToolCall::Dynamic { status, .. } => *status = next_status,
        }
    }
}

#[derive(Clone)]
pub struct FileChangeSummary {
    pub path: String,
    pub kind: FileChangeKind,
    pub additions: usize,
    pub deletions: usize,
}

#[derive(Clone)]
pub enum FileChangeKind {
    Add,
    Delete,
    Update { move_path: Option<String> },
}

#[derive(Clone, Copy, PartialEq, Eq)]
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

    pub fn set_status(&mut self, status: impl Into<String>) {
        self.status = status.into();
    }
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
