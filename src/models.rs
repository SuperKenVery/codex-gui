use gpui::SharedString;

#[derive(Clone)]
pub struct Project {
    pub name: SharedString,
    pub path: SharedString,
    pub chats: Vec<Chat>,
}

#[derive(Clone)]
pub struct Chat {
    pub id: String,
    pub title: SharedString,
    pub subtitle: SharedString,
    pub messages: Vec<Message>,
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
