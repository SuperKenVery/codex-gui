use crate::models::Message;
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
}

impl MessageState {
    pub fn new(message: Message) -> Self {
        Self { message }
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
