use crate::bridge::{BridgeCommand, BridgeEvent, empty_chat, start_app_server_bridge};
use crate::gui::{
    BridgeState, ChatPanel, ChatState, GuiState, MessageState, ProjectState, SideChat, Sidebar,
    UiState,
};
use crate::models::{Chat, Message, StreamState, ToolCall, ToolStatus};
use crate::workspace::workspace_path;
use gpui::{
    Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Task, Window, div,
    prelude::*,
};
use gpui_component::ActiveTheme as _;
use std::{
    sync::mpsc::{Receiver, Sender},
    time::Duration,
};

pub struct CodexGui {
    state: Entity<GuiState>,
    ui_state: Entity<UiState>,
    bridge_state: Entity<BridgeState>,
    bridge_tx: Option<Sender<BridgeCommand>>,
    bridge_rx: Receiver<BridgeEvent>,
    pending_turn_text: Option<String>,
    sidebar: Entity<Sidebar>,
    chat_panel: Entity<ChatPanel>,
    side_chat: Entity<SideChat>,
    _bridge_task: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl CodexGui {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (bridge_tx, bridge_rx) = start_app_server_bridge();
        let initial_project =
            cx.new(|_| ProjectState::new("codex-gui".into(), workspace_path().into(), Vec::new()));
        let state = cx.new(|_| GuiState::new(vec![initial_project]));
        let ui_state = cx.new(|_| UiState::new());
        let bridge_state = cx.new(|_| BridgeState::new());
        let parent = cx.entity().downgrade();
        let sidebar = cx.new(|cx| Sidebar::new(parent.clone(), state.clone(), cx));
        let chat_panel = cx.new(|cx| {
            ChatPanel::new(
                parent.clone(),
                state.clone(),
                bridge_state.clone(),
                window,
                cx,
            )
        });
        let side_chat = cx.new(|cx| SideChat::new(state.clone(), cx));

        let bridge_task = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                if this
                    .update(cx, |view, cx| view.drain_bridge_events(cx))
                    .is_err()
                {
                    break;
                }
            }
        });

        Self {
            state,
            ui_state,
            bridge_state,
            bridge_tx: Some(bridge_tx),
            bridge_rx,
            pending_turn_text: None,
            sidebar,
            chat_panel,
            side_chat,
            _bridge_task: bridge_task,
            _subscriptions: Vec::new(),
        }
    }

    pub(crate) fn select_project(&mut self, index: usize, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| {
            state.active_project = index;
            state.active_chat = 0;
            cx.notify();
        });
    }

    pub(crate) fn select_chat(&mut self, index: usize, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| {
            state.active_chat = index;
            cx.notify();
        });
    }

    pub(crate) fn fork_chat(&mut self, cx: &mut Context<Self>) {
        let Some(thread_id) = self
            .active_chat_entity(cx)
            .map(|chat| chat.read(cx).id.clone())
        else {
            return;
        };
        self.send_bridge(BridgeCommand::ForkThread { thread_id }, cx);
        self.set_bridge_status("forking thread", cx);
    }

    pub(crate) fn start_thread(&mut self, cx: &mut Context<Self>) {
        self.send_bridge(
            BridgeCommand::StartThread {
                cwd: workspace_path(),
            },
            cx,
        );
        self.set_bridge_status("starting thread", cx);
    }

    pub(crate) fn send_turn_text(&mut self, text: String, cx: &mut Context<Self>) {
        let Some(thread_id) = self
            .active_chat_entity(cx)
            .map(|chat| chat.read(cx).id.clone())
        else {
            self.pending_turn_text = Some(text);
            self.start_thread(cx);
            return;
        };
        if thread_id == "empty" {
            self.pending_turn_text = Some(text);
            self.start_thread(cx);
            return;
        }
        self.send_bridge(BridgeCommand::SendTurn { thread_id, text }, cx);
        self.set_bridge_status("turn running", cx);
    }

    pub(crate) fn toggle_side_chat(&mut self, cx: &mut Context<Self>) {
        self.ui_state.update(cx, |state, cx| {
            state.side_chat_open = !state.side_chat_open;
            cx.notify();
        });
        cx.notify();
    }

    fn send_bridge(&mut self, command: BridgeCommand, cx: &mut Context<Self>) {
        if let Some(tx) = &self.bridge_tx {
            if tx.send(command).is_err() {
                self.set_bridge_status("codex app-server writer stopped", cx);
            }
        }
    }

    fn set_bridge_status(&self, status: impl Into<String>, cx: &mut Context<Self>) {
        self.bridge_state.update(cx, |state, cx| {
            state.status = status.into();
            cx.notify();
        });
    }

    fn drain_bridge_events(&mut self, cx: &mut Context<Self>) {
        while let Ok(event) = self.bridge_rx.try_recv() {
            self.apply_bridge_event(event, cx);
        }
    }

    fn apply_bridge_event(&mut self, event: BridgeEvent, cx: &mut Context<Self>) {
        match event {
            BridgeEvent::Status(status) => self.set_bridge_status(status, cx),
            BridgeEvent::ThreadsLoaded(chats) => {
                let chats = if chats.is_empty() {
                    vec![empty_chat()]
                } else {
                    chats
                };
                let chats = chats
                    .into_iter()
                    .map(|chat| chat_entity_from_model(chat, cx))
                    .collect::<Vec<_>>();
                if let Some(project) = self.active_project_entity(cx) {
                    project.update(cx, |project, cx| {
                        project.chats = chats;
                        cx.notify();
                    });
                }
                self.state.update(cx, |state, cx| {
                    state.active_chat = 0;
                    cx.notify();
                });
                self.set_bridge_status("connected to codex app-server", cx);
            }
            BridgeEvent::ThreadStarted(chat) | BridgeEvent::ThreadForked(chat) => {
                let thread_id = chat.id.clone();
                let chat = chat_entity_from_model(chat, cx);
                if let Some(project) = self.active_project_entity(cx) {
                    project.update(cx, |project, cx| {
                        upsert_chat(project, chat, &thread_id, cx);
                        cx.notify();
                    });
                }
                self.state.update(cx, |state, cx| {
                    state.active_chat = 0;
                    cx.notify();
                });
                self.set_bridge_status("thread ready", cx);
                if let Some(text) = self.pending_turn_text.take() {
                    self.send_bridge(BridgeCommand::SendTurn { thread_id, text }, cx);
                    self.set_bridge_status("turn running", cx);
                }
            }
            BridgeEvent::TurnStarted { thread_id } => {
                self.set_bridge_status("turn running", cx);
                self.append_message(
                    &thread_id,
                    Message::Commentary("Codex accepted the turn.".into()),
                    cx,
                );
            }
            BridgeEvent::UserMessage { thread_id, text } => {
                if !text.is_empty() {
                    self.append_message(&thread_id, Message::User(text), cx);
                }
            }
            BridgeEvent::AgentMessageStarted {
                thread_id,
                item_id,
                text,
            } => {
                self.append_message(
                    &thread_id,
                    Message::Assistant {
                        id: item_id,
                        body: text,
                        state: StreamState::Streaming,
                        tools: Vec::new(),
                    },
                    cx,
                );
            }
            BridgeEvent::AgentMessageDelta {
                thread_id,
                item_id,
                delta,
            } => self.append_agent_delta(&thread_id, &item_id, &delta, cx),
            BridgeEvent::ToolStarted { thread_id, tool } => {
                self.append_or_update_tool(&thread_id, tool, cx);
            }
            BridgeEvent::ToolOutputDelta {
                thread_id,
                item_id,
                delta,
            } => self.append_tool_output_delta(&thread_id, &item_id, &delta, cx),
            BridgeEvent::ItemCompleted { thread_id, item_id } => {
                self.mark_item_complete(&thread_id, &item_id, cx);
            }
            BridgeEvent::Error(message) => {
                self.set_bridge_status("codex app-server error", cx);
                if let Some(chat) = self.active_chat_entity(cx) {
                    let thread_id = chat.read(cx).id.clone();
                    self.append_message(&thread_id, Message::Commentary(message), cx);
                } else if let Some(project) = self.active_project_entity(cx) {
                    let chat = chat_entity_from_model(
                        Chat {
                            id: "bridge-error".into(),
                            title: "Bridge error".into(),
                            subtitle: message.clone().into(),
                            messages: vec![Message::Commentary(message)],
                        },
                        cx,
                    );
                    project.update(cx, |project, cx| {
                        project.chats.push(chat);
                        cx.notify();
                    });
                }
            }
        }
    }

    fn active_project_entity(&self, cx: &mut Context<Self>) -> Option<Entity<ProjectState>> {
        self.state.read(cx).active_project()
    }

    fn active_chat_entity(&self, cx: &mut Context<Self>) -> Option<Entity<ChatState>> {
        let (project, active_chat) = {
            let state = self.state.read(cx);
            (state.active_project(), state.active_chat)
        };
        project.and_then(|project| {
            let chats = project.read(cx).chats.clone();
            chats.get(active_chat).cloned()
        })
    }

    fn find_chat_entity(
        &self,
        thread_id: &str,
        cx: &mut Context<Self>,
    ) -> Option<Entity<ChatState>> {
        let project = self.active_project_entity(cx)?;
        let chats = project.read(cx).chats.clone();
        for chat in chats {
            let is_match = chat.read(cx).id == thread_id;
            if is_match {
                return Some(chat);
            }
        }
        None
    }

    fn append_message(&self, thread_id: &str, message: Message, cx: &mut Context<Self>) {
        let Some(chat) = self.find_chat_entity(thread_id, cx) else {
            return;
        };
        let message = cx.new(|_| MessageState::new(message));
        chat.update(cx, |chat, cx| {
            chat.messages.push(message);
            cx.notify();
        });
    }

    fn append_agent_delta(
        &self,
        thread_id: &str,
        item_id: &str,
        delta: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(message) = self.find_assistant_message(thread_id, item_id, cx) else {
            self.append_message(
                thread_id,
                Message::Assistant {
                    id: item_id.to_string(),
                    body: delta.to_string(),
                    state: StreamState::Streaming,
                    tools: Vec::new(),
                },
                cx,
            );
            return;
        };
        message.update(cx, |message, cx| {
            if let Message::Assistant { body, state, .. } = &mut message.message {
                body.push_str(delta);
                *state = StreamState::Streaming;
                cx.notify();
            }
        });
    }

    fn append_or_update_tool(&self, thread_id: &str, tool: ToolCall, cx: &mut Context<Self>) {
        if let Some(message) = self.find_latest_assistant_message(thread_id, cx) {
            message.update(cx, |message, cx| {
                if let Message::Assistant { tools, .. } = &mut message.message {
                    if let Some(existing) = tools.iter_mut().find(|existing| existing.id == tool.id)
                    {
                        *existing = tool;
                    } else {
                        tools.push(tool);
                    }
                    cx.notify();
                }
            });
        } else {
            self.append_message(
                thread_id,
                Message::Assistant {
                    id: format!("tool-group-{}", tool.id),
                    body: "Codex is using tools.".into(),
                    state: StreamState::Streaming,
                    tools: vec![tool],
                },
                cx,
            );
        }
    }

    fn append_tool_output_delta(
        &self,
        thread_id: &str,
        item_id: &str,
        delta: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(message) = self.find_tool_message(thread_id, item_id, cx) else {
            return;
        };
        message.update(cx, |message, cx| {
            if let Message::Assistant { tools, .. } = &mut message.message {
                if let Some(tool) = tools.iter_mut().find(|tool| tool.id == item_id) {
                    tool.detail.push_str(delta);
                    cx.notify();
                }
            }
        });
    }

    fn mark_item_complete(&self, thread_id: &str, item_id: &str, cx: &mut Context<Self>) {
        if let Some(message) = self.find_assistant_message(thread_id, item_id, cx) {
            message.update(cx, |message, cx| {
                if let Message::Assistant { state, .. } = &mut message.message {
                    *state = StreamState::Complete;
                    cx.notify();
                }
            });
            return;
        }

        if let Some(message) = self.find_tool_message(thread_id, item_id, cx) {
            message.update(cx, |message, cx| {
                if let Message::Assistant { tools, .. } = &mut message.message {
                    if let Some(tool) = tools.iter_mut().find(|tool| tool.id == item_id) {
                        tool.status = ToolStatus::Done;
                        cx.notify();
                    }
                }
            });
        }
    }

    fn find_assistant_message(
        &self,
        thread_id: &str,
        item_id: &str,
        cx: &mut Context<Self>,
    ) -> Option<Entity<MessageState>> {
        let chat = self.find_chat_entity(thread_id, cx)?;
        let messages = chat.read(cx).messages.clone();
        for message in messages.into_iter().rev() {
            let is_match = matches!(
                &message.read(cx).message,
                Message::Assistant { id, .. } if id == item_id
            );
            if is_match {
                return Some(message);
            }
        }
        None
    }

    fn find_latest_assistant_message(
        &self,
        thread_id: &str,
        cx: &mut Context<Self>,
    ) -> Option<Entity<MessageState>> {
        let chat = self.find_chat_entity(thread_id, cx)?;
        let messages = chat.read(cx).messages.clone();
        for message in messages.into_iter().rev() {
            let is_match = matches!(&message.read(cx).message, Message::Assistant { .. });
            if is_match {
                return Some(message);
            }
        }
        None
    }

    fn find_tool_message(
        &self,
        thread_id: &str,
        item_id: &str,
        cx: &mut Context<Self>,
    ) -> Option<Entity<MessageState>> {
        let chat = self.find_chat_entity(thread_id, cx)?;
        let messages = chat.read(cx).messages.clone();
        for message in messages.into_iter().rev() {
            let is_match = matches!(
                &message.read(cx).message,
                Message::Assistant { tools, .. } if tools.iter().any(|tool| tool.id == item_id)
            );
            if is_match {
                return Some(message);
            }
        }
        None
    }
}

impl Render for CodexGui {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.drain_bridge_events(cx);
        let side_chat_open = self.ui_state.read(cx).side_chat_open;

        div()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .font_family(".SystemUIFont")
            .child(
                div()
                    .flex()
                    .size_full()
                    .child(self.sidebar.clone())
                    .child(self.chat_panel.clone())
                    .when(side_chat_open, |this| this.child(self.side_chat.clone())),
            )
    }
}

fn chat_entity_from_model(chat: Chat, cx: &mut Context<CodexGui>) -> Entity<ChatState> {
    let messages = chat
        .messages
        .into_iter()
        .map(|message| cx.new(|_| MessageState::new(message)))
        .collect();
    cx.new(|_| ChatState::new(chat.id, chat.title, chat.subtitle, messages))
}

fn upsert_chat(
    project: &mut ProjectState,
    chat: Entity<ChatState>,
    chat_id: &str,
    cx: &mut Context<ProjectState>,
) {
    for index in 0..project.chats.len() {
        let is_match = project.chats[index].read(cx).id == chat_id;
        if is_match {
            project.chats[index] = chat;
            return;
        }
    }
    project.chats.insert(0, chat);
}
