use std::{
    collections::{HashMap, hash_map::DefaultHasher},
    hash::{Hash as _, Hasher as _},
    time::Instant,
};

use codex_app_server_protocol::{FileUpdateChange, Thread, ThreadItem, ThreadStatus, Turn};
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
    pub thread: Option<Thread>,
    pub messages: Vec<Entity<MessageState>>,
    item_locations: HashMap<String, ThreadItemLocation>,
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
            thread: None,
            messages,
            item_locations: HashMap::new(),
        }
    }

    pub fn from_thread(
        thread: Thread,
        title: SharedString,
        subtitle: SharedString,
        messages: Vec<Entity<MessageState>>,
    ) -> Self {
        let id = thread.id.clone();
        let item_locations = thread_item_locations(&thread);
        Self {
            id,
            title,
            subtitle,
            thread: Some(thread),
            messages,
            item_locations,
        }
    }

    pub fn append_message(&mut self, message: Entity<MessageState>) {
        self.messages.push(message);
    }

    pub fn set_title(&mut self, title: String) {
        if let Some(thread) = &mut self.thread {
            thread.name = Some(title.clone());
        }
        self.title = title.into();
    }

    pub fn set_thread_status(&mut self, status: ThreadStatus) {
        if let Some(thread) = &mut self.thread {
            thread.status = status;
        }
    }

    pub fn upsert_turn(&mut self, turn: Turn) {
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

    pub fn append_thread_item(&mut self, turn_id: Option<&str>, item: ThreadItem) {
        let Some(thread) = &mut self.thread else {
            return;
        };
        let item_id = item.id().to_string();
        let mut changed = false;
        if let Some(turn_id) = turn_id
            && let Some(turn) = thread.turns.iter_mut().find(|turn| turn.id == turn_id)
        {
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

    pub fn item_for_state(&self, state: &MessageState) -> Option<&ThreadItem> {
        match &state.key {
            HistoryKey::Item(item_id) | HistoryKey::ToolGroup(item_id) => self.thread_item(item_id),
            HistoryKey::Notice(_) => None,
        }
    }

    pub fn has_done_tools_for_state(&self, state: &MessageState) -> bool {
        let tools = self.tools_for_state(state);
        !tools.is_empty() && tools.iter().all(|tool| tool_item_done(tool))
    }

    pub fn item_is_agent_message(&self, item_id: &str) -> bool {
        matches!(
            self.thread_item(item_id),
            Some(ThreadItem::AgentMessage { .. })
        )
    }

    pub fn message_has_tool(&self, state: &MessageState, item_id: &str) -> bool {
        self.tools_for_state(state)
            .iter()
            .any(|tool| tool.id() == item_id)
    }

    pub fn tools_for_state(&self, state: &MessageState) -> Vec<&ThreadItem> {
        match (&state.key, state.kind) {
            (HistoryKey::Item(item_id), HistoryEntryKind::Assistant) => {
                self.attached_tools_after(item_id)
            }
            (HistoryKey::ToolGroup(first_tool_id), HistoryEntryKind::ToolGroup) => {
                self.tool_group_from(first_tool_id)
            }
            _ => Vec::new(),
        }
    }

    fn attached_tools_after(&self, item_id: &str) -> Vec<&ThreadItem> {
        let Some((turn_index, item_index)) = self.thread_item_position(item_id) else {
            return Vec::new();
        };
        self.thread
            .as_ref()
            .and_then(|thread| thread.turns.get(turn_index))
            .map(|turn| {
                turn.items
                    .iter()
                    .skip(item_index + 1)
                    .take_while(|item| is_tool_item(item))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn tool_group_from(&self, first_tool_id: &str) -> Vec<&ThreadItem> {
        let Some((turn_index, item_index)) = self.thread_item_position(first_tool_id) else {
            return Vec::new();
        };
        self.thread
            .as_ref()
            .and_then(|thread| thread.turns.get(turn_index))
            .map(|turn| {
                turn.items
                    .iter()
                    .skip(item_index)
                    .take_while(|item| is_tool_item(item))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn thread_item(&self, item_id: &str) -> Option<&ThreadItem> {
        let location = self.item_locations.get(item_id)?;
        self.thread
            .as_ref()?
            .turns
            .get(location.turn_index)?
            .items
            .get(location.item_index)
    }

    fn thread_item_position(&self, item_id: &str) -> Option<(usize, usize)> {
        self.item_locations
            .get(item_id)
            .map(|location| (location.turn_index, location.item_index))
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

pub struct MessageState {
    pub key: HistoryKey,
    pub kind: HistoryEntryKind,
    pub rendered_body: String,
    pub stream_state: StreamState,
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
    pub fn notice(body: String, cx: &mut Context<Self>) -> Self {
        Self::new_with_key(
            HistoryKey::Notice(format!("notice-{}", stable_text_id(&body))),
            HistoryEntryKind::Notice,
            Some(body),
            StreamState::Complete,
            cx,
        )
    }

    pub fn item(
        key: HistoryKey,
        kind: HistoryEntryKind,
        body: Option<String>,
        stream_state: StreamState,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new_with_key(key, kind, body, stream_state, cx)
    }

    pub fn new_with_key(
        key: HistoryKey,
        kind: HistoryEntryKind,
        body: Option<String>,
        stream_state: StreamState,
        cx: &mut Context<Self>,
    ) -> Self {
        let now = Instant::now();
        let rendered_body = body.unwrap_or_default();
        let body_view = (!rendered_body.is_empty())
            .then(|| cx.new(|cx| TextViewState::markdown(&rendered_body, cx)));
        let zed_markdown = (!rendered_body.is_empty())
            .then(|| cx.new(|cx| ZedMarkdown::new(rendered_body.clone().into(), None, None, cx)));
        Self {
            key,
            kind,
            rendered_body,
            stream_state,
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

    pub fn mark_streaming(&mut self) {
        self.stream_state = StreamState::Streaming;
        self.touch();
    }

    pub fn mark_complete(&mut self, cx: &mut Context<Self>) {
        self.stream_state = StreamState::Complete;
        self.touch();
        self.sync_body_view(cx);
    }

    pub fn sync_body_view(&mut self, cx: &mut Context<Self>) {
        if self.rendered_body.is_empty() {
            self.body_view = None;
            self.zed_markdown = None;
            return;
        }
        if let Some(body_view) = &self.body_view {
            body_view.update(cx, |body_view, cx| {
                body_view.set_text(&self.rendered_body, cx)
            });
        } else {
            self.body_view = Some(cx.new(|cx| TextViewState::markdown(&self.rendered_body, cx)));
        }
        if let Some(zed_markdown) = &self.zed_markdown {
            zed_markdown.update(cx, |markdown, cx| {
                markdown.replace(self.rendered_body.clone(), cx)
            });
        } else {
            self.zed_markdown = Some(
                cx.new(|cx| ZedMarkdown::new(self.rendered_body.clone().into(), None, None, cx)),
            );
        }
    }

    pub fn append_body_view_delta(&mut self, delta: &str, cx: &mut Context<Self>) {
        if delta.is_empty() {
            return;
        }

        self.rendered_body.push_str(delta);
        if let Some(body_view) = &self.body_view {
            body_view.update(cx, |body_view, cx| body_view.push_str(delta, cx));
        } else {
            self.body_view = Some(cx.new(|cx| TextViewState::markdown(&self.rendered_body, cx)));
        }
        if let Some(zed_markdown) = &self.zed_markdown {
            zed_markdown.update(cx, |markdown, cx| markdown.append(delta, cx));
        } else {
            self.zed_markdown = Some(
                cx.new(|cx| ZedMarkdown::new(self.rendered_body.clone().into(), None, None, cx)),
            );
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum HistoryKey {
    Item(String),
    ToolGroup(String),
    Notice(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HistoryEntryKind {
    User,
    Assistant,
    ToolGroup,
    Notice,
}

fn stable_text_id(text: &str) -> String {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[derive(Clone, Copy)]
pub enum StreamState {
    Complete,
    Streaming,
}

fn is_tool_item(item: &ThreadItem) -> bool {
    match item {
        ThreadItem::CommandExecution { .. }
        | ThreadItem::FileChange { .. }
        | ThreadItem::McpToolCall { .. }
        | ThreadItem::DynamicToolCall { .. } => true,
        _ => false,
    }
}

fn tool_item_done(item: &ThreadItem) -> bool {
    match item {
        ThreadItem::CommandExecution { status, .. } => !matches!(
            status,
            codex_app_server_protocol::CommandExecutionStatus::InProgress
        ),
        ThreadItem::FileChange { status, .. } => !matches!(
            status,
            codex_app_server_protocol::PatchApplyStatus::InProgress
        ),
        ThreadItem::McpToolCall { status, .. } => !matches!(
            status,
            codex_app_server_protocol::McpToolCallStatus::InProgress
        ),
        ThreadItem::DynamicToolCall { status, .. } => !matches!(
            status,
            codex_app_server_protocol::DynamicToolCallStatus::InProgress
        ),
        _ => false,
    }
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
