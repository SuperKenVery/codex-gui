use std::time::Instant;

use gpui::{Entity, SharedString};

pub struct GuiState {
    pub projects: Vec<Entity<ProjectState>>,
    pub active_project: usize,
    pub active_chat: usize,
}

impl GuiState {
    pub fn new(projects: Vec<Entity<ProjectState>>) -> Self {
        Self {
            projects,
            active_project: 0,
            active_chat: 0,
        }
    }

    pub fn active_project(&self) -> Option<Entity<ProjectState>> {
        self.projects.get(self.active_project).cloned()
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
