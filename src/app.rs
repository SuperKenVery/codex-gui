use crate::bridge::{BridgeCommand, BridgeEvent, empty_chat, start_app_server_bridge};
use crate::models::{Chat, Message, Project, StreamState, ToolCall, ToolStatus};
use crate::workspace::workspace_path;
use gpui::{
    Context, Entity, IntoElement, ParentElement, Render, SharedString, Styled, Subscription, Task,
    Window, div, prelude::*, px, rgb,
};
use gpui_component::{
    ActiveTheme as _, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    input::{Input, InputEvent, InputState},
    v_flex,
};
use std::{
    sync::mpsc::{Receiver, Sender},
    time::Duration,
};

pub struct CodexGui {
    projects: Vec<Project>,
    active_project: usize,
    active_chat: usize,
    side_chat_open: bool,
    bridge_status: String,
    bridge_tx: Option<Sender<BridgeCommand>>,
    bridge_rx: Receiver<BridgeEvent>,
    composer_input: Entity<InputState>,
    pending_turn_text: Option<String>,
    _bridge_task: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl CodexGui {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (bridge_tx, bridge_rx) = start_app_server_bridge();
        let composer_input = cx.new(|cx| {
            InputState::new(window, cx)
                .auto_grow(1, 5)
                .submit_on_enter(true)
                .placeholder("Ask Codex to change, explain, or inspect this project")
        });
        let subscriptions =
            vec![
                cx.subscribe_in(&composer_input, window, |view, _, event, window, cx| {
                    if matches!(event, InputEvent::PressEnter { shift: false, .. }) {
                        view.send_composer_turn(window, cx);
                    }
                }),
            ];
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
            projects: vec![Project {
                name: "codex-gui".into(),
                path: workspace_path().into(),
                chats: Vec::new(),
            }],
            active_project: 0,
            active_chat: 0,
            side_chat_open: false,
            bridge_status: "starting codex app-server".into(),
            bridge_tx: Some(bridge_tx),
            bridge_rx,
            composer_input,
            pending_turn_text: None,
            _bridge_task: bridge_task,
            _subscriptions: subscriptions,
        }
    }

    fn active_project(&self) -> &Project {
        &self.projects[self.active_project]
    }

    fn active_project_mut(&mut self) -> &mut Project {
        &mut self.projects[self.active_project]
    }

    fn active_chat(&self) -> Option<&Chat> {
        self.active_project().chats.get(self.active_chat)
    }

    fn select_project(&mut self, index: usize, cx: &mut Context<Self>) {
        self.active_project = index;
        self.active_chat = 0;
        cx.notify();
    }

    fn select_chat(&mut self, index: usize, cx: &mut Context<Self>) {
        self.active_chat = index;
        cx.notify();
    }

    fn fork_chat(&mut self, cx: &mut Context<Self>) {
        let Some(thread_id) = self.active_chat().map(|chat| chat.id.clone()) else {
            return;
        };
        self.send_bridge(BridgeCommand::ForkThread { thread_id });
        self.bridge_status = "forking thread".into();
        cx.notify();
    }

    fn start_thread(&mut self, cx: &mut Context<Self>) {
        self.send_bridge(BridgeCommand::StartThread {
            cwd: workspace_path(),
        });
        self.bridge_status = "starting thread".into();
        cx.notify();
    }

    fn send_composer_turn(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.composer_input.update(cx, |input, cx| {
            let text = input.value().trim().to_string();
            if !text.is_empty() {
                input.set_value("", window, cx);
            }
            text
        });
        if text.is_empty() {
            return;
        }
        self.send_turn_text(text, cx);
    }

    fn send_turn_text(&mut self, text: String, cx: &mut Context<Self>) {
        let Some(thread_id) = self.active_chat().map(|chat| chat.id.clone()) else {
            self.pending_turn_text = Some(text);
            self.start_thread(cx);
            return;
        };
        if thread_id == "empty" {
            self.pending_turn_text = Some(text);
            self.start_thread(cx);
            return;
        }
        self.send_bridge(BridgeCommand::SendTurn { thread_id, text });
        self.bridge_status = "turn running".into();
        cx.notify();
    }

    fn toggle_side_chat(&mut self, cx: &mut Context<Self>) {
        self.side_chat_open = !self.side_chat_open;
        cx.notify();
    }

    fn send_bridge(&mut self, command: BridgeCommand) {
        if let Some(tx) = &self.bridge_tx {
            if tx.send(command).is_err() {
                self.bridge_status = "codex app-server writer stopped".into();
            }
        }
    }

    fn drain_bridge_events(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        while let Ok(event) = self.bridge_rx.try_recv() {
            changed = true;
            self.apply_bridge_event(event);
        }
        if changed {
            cx.notify();
        }
    }

    fn apply_bridge_event(&mut self, event: BridgeEvent) {
        match event {
            BridgeEvent::Status(status) => self.bridge_status = status,
            BridgeEvent::ThreadsLoaded(chats) => {
                let project = self.active_project_mut();
                project.chats = chats;
                if project.chats.is_empty() {
                    project.chats.push(empty_chat());
                }
                self.active_chat = 0;
                self.bridge_status = "connected to codex app-server".into();
            }
            BridgeEvent::ThreadStarted(chat) | BridgeEvent::ThreadForked(chat) => {
                let thread_id = chat.id.clone();
                self.upsert_chat(chat);
                self.active_chat = 0;
                self.bridge_status = "thread ready".into();
                if let Some(text) = self.pending_turn_text.take() {
                    self.send_bridge(BridgeCommand::SendTurn { thread_id, text });
                    self.bridge_status = "turn running".into();
                }
            }
            BridgeEvent::TurnStarted { thread_id } => {
                self.bridge_status = "turn running".into();
                self.append_message(
                    &thread_id,
                    Message::Commentary("Codex accepted the turn.".into()),
                );
            }
            BridgeEvent::UserMessage { thread_id, text } => {
                if !text.is_empty() {
                    self.append_message(&thread_id, Message::User(text));
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
                );
            }
            BridgeEvent::AgentMessageDelta {
                thread_id,
                item_id,
                delta,
            } => self.append_agent_delta(&thread_id, &item_id, &delta),
            BridgeEvent::ToolStarted { thread_id, tool } => {
                self.append_or_update_tool(&thread_id, tool);
            }
            BridgeEvent::ToolOutputDelta {
                thread_id,
                item_id,
                delta,
            } => self.append_tool_output_delta(&thread_id, &item_id, &delta),
            BridgeEvent::ItemCompleted { thread_id, item_id } => {
                self.mark_item_complete(&thread_id, &item_id);
            }
            BridgeEvent::Error(message) => {
                self.bridge_status = "codex app-server error".into();
                let thread_id = self.active_chat().map(|chat| chat.id.clone());
                if let Some(thread_id) = thread_id {
                    self.append_message(&thread_id, Message::Commentary(message));
                } else {
                    self.active_project_mut().chats.push(Chat {
                        id: "bridge-error".into(),
                        title: "Bridge error".into(),
                        subtitle: message.clone().into(),
                        messages: vec![Message::Commentary(message)],
                    });
                }
            }
        }
    }

    fn upsert_chat(&mut self, chat: Chat) {
        let project = self.active_project_mut();
        if let Some(existing) = project
            .chats
            .iter_mut()
            .find(|existing| existing.id == chat.id)
        {
            *existing = chat;
        } else {
            project.chats.insert(0, chat);
        }
    }

    fn append_message(&mut self, thread_id: &str, message: Message) {
        if let Some(chat) = self.find_chat_mut(thread_id) {
            chat.messages.push(message);
        }
    }

    fn append_agent_delta(&mut self, thread_id: &str, item_id: &str, delta: &str) {
        let Some(chat) = self.find_chat_mut(thread_id) else {
            return;
        };
        if let Some(Message::Assistant { body, state, .. }) = chat
            .messages
            .iter_mut()
            .rev()
            .find(|message| matches!(message, Message::Assistant { id, .. } if id == item_id))
        {
            body.push_str(delta);
            *state = StreamState::Streaming;
        } else {
            chat.messages.push(Message::Assistant {
                id: item_id.to_string(),
                body: delta.to_string(),
                state: StreamState::Streaming,
                tools: Vec::new(),
            });
        }
    }

    fn append_or_update_tool(&mut self, thread_id: &str, tool: ToolCall) {
        let Some(chat) = self.find_chat_mut(thread_id) else {
            return;
        };
        if let Some(Message::Assistant { tools, .. }) = chat
            .messages
            .iter_mut()
            .rev()
            .find(|message| matches!(message, Message::Assistant { .. }))
        {
            if let Some(existing) = tools.iter_mut().find(|existing| existing.id == tool.id) {
                *existing = tool;
            } else {
                tools.push(tool);
            }
        } else {
            chat.messages.push(Message::Assistant {
                id: format!("tool-group-{}", tool.id),
                body: "Codex is using tools.".into(),
                state: StreamState::Streaming,
                tools: vec![tool],
            });
        }
    }

    fn append_tool_output_delta(&mut self, thread_id: &str, item_id: &str, delta: &str) {
        let Some(chat) = self.find_chat_mut(thread_id) else {
            return;
        };
        for message in chat.messages.iter_mut().rev() {
            if let Message::Assistant { tools, .. } = message {
                if let Some(tool) = tools.iter_mut().find(|tool| tool.id == item_id) {
                    tool.detail.push_str(delta);
                    return;
                }
            }
        }
    }

    fn mark_item_complete(&mut self, thread_id: &str, item_id: &str) {
        let Some(chat) = self.find_chat_mut(thread_id) else {
            return;
        };
        for message in &mut chat.messages {
            match message {
                Message::Assistant { id, state, .. } if id == item_id => {
                    *state = StreamState::Complete;
                }
                Message::Assistant { tools, .. } => {
                    if let Some(tool) = tools.iter_mut().find(|tool| tool.id == item_id) {
                        tool.status = ToolStatus::Done;
                    }
                }
                _ => {}
            }
        }
    }

    fn find_chat_mut(&mut self, thread_id: &str) -> Option<&mut Chat> {
        self.active_project_mut()
            .chats
            .iter_mut()
            .find(|chat| chat.id == thread_id)
    }
}

impl Render for CodexGui {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.drain_bridge_events(cx);

        div()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .font_family(".SystemUIFont")
            .child(
                div()
                    .flex()
                    .size_full()
                    .child(self.render_sidebar(cx))
                    .child(self.render_chat(cx))
                    .when(self.side_chat_open, |this| {
                        this.child(self.render_side_chat(cx))
                    }),
            )
    }
}

impl CodexGui {
    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let project_items =
            self.projects
                .iter()
                .enumerate()
                .fold(v_flex().gap_1(), |list, (index, project)| {
                    let selected = index == self.active_project;
                    list.child(
                        nav_item(format!("project-{index}"), project.name.clone(), selected)
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(0x8d95a5))
                                    .child(project.path.clone()),
                            )
                            .on_click(
                                cx.listener(move |view, _, _, cx| view.select_project(index, cx)),
                            ),
                    )
                });

        let chat_items = self.active_project().chats.iter().enumerate().fold(
            v_flex().gap_1(),
            |list, (index, chat)| {
                let selected = index == self.active_chat;
                list.child(
                    nav_item(format!("chat-{index}"), chat.title.clone(), selected)
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(0x9aa3b5))
                                .child(chat.subtitle.clone()),
                        )
                        .on_click(cx.listener(move |view, _, _, cx| view.select_chat(index, cx))),
                )
            },
        );

        div()
            .w(px(286.))
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().sidebar)
            .p_3()
            .gap_4()
            .child(section_label("Projects"))
            .child(project_items)
            .child(section_label("Codex Threads"))
            .child(chat_items)
            .child(
                div()
                    .mt_auto()
                    .flex()
                    .gap_2()
                    .child(
                        command_button("start-thread", "New")
                            .primary()
                            .on_click(cx.listener(|view, _, _, cx| view.start_thread(cx))),
                    )
                    .child(
                        command_button("fork-chat", "Fork")
                            .on_click(cx.listener(|view, _, _, cx| view.fork_chat(cx))),
                    )
                    .child(
                        command_button("toggle-side-chat", "Side")
                            .on_click(cx.listener(|view, _, _, cx| view.toggle_side_chat(cx))),
                    ),
            )
    }

    fn render_chat(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let messages = self
            .active_chat()
            .map(|chat| {
                chat.messages.iter().fold(
                    div()
                        .id("message-list")
                        .flex()
                        .flex_col()
                        .size_full()
                        .gap_3()
                        .overflow_y_scroll(),
                    |list, message| list.child(render_message(message)),
                )
            })
            .unwrap_or_else(|| {
                div()
                    .id("message-list")
                    .flex()
                    .flex_col()
                    .size_full()
                    .gap_3()
                    .overflow_y_scroll()
                    .child(render_message(&Message::Commentary(
                        "Loading Codex threads from the app server.".into(),
                    )))
            });

        let (title, subtitle) = self
            .active_chat()
            .map(|chat| (chat.title.clone(), chat.subtitle.clone()))
            .unwrap_or_else(|| ("No thread selected".into(), "Start a Codex thread".into()));

        div()
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .child(
                div()
                    .h(px(58.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .px_5()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(title),
                            )
                            .child(div().text_xs().text_color(rgb(0x8f98a8)).child(subtitle)),
                    )
                    .child(status_pill(self.bridge_status.clone())),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .p_5()
                    .child(messages),
            )
            .child(composer(self.composer_input.clone(), cx))
    }

    fn render_side_chat(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let thread = self
            .active_chat()
            .map(|chat| chat.title.clone())
            .unwrap_or_else(|| "No thread".into());

        div()
            .w(px(340.))
            .h_full()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(rgb(0x252933))
            .bg(cx.theme().sidebar)
            .child(
                div()
                    .h(px(58.))
                    .flex()
                    .items_center()
                    .px_4()
                    .border_b_1()
                    .border_color(rgb(0x252933))
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("Side Chat"),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(0x2f3542))
                            .bg(rgb(0x1a1f29))
                            .p_3()
                            .child(format!("Temporary view of {thread}.")),
                    )
                    .child(render_message(&Message::Commentary(
                        "Side chats remain a UI-only view until promoted through thread/fork."
                            .into(),
                    ))),
            )
    }
}

fn section_label(text: &'static str) -> impl IntoElement {
    div()
        .text_xs()
        .text_color(rgb(0x717b8f))
        .child(text.to_ascii_uppercase())
}

fn nav_item(id: impl Into<gpui::ElementId>, label: SharedString, selected: bool) -> Button {
    Button::new(id).ghost().selected(selected).w_full().child(
        v_flex()
            .items_start()
            .gap_1()
            .child(div().text_sm().child(label)),
    )
}

fn command_button(id: &'static str, label: &'static str) -> Button {
    Button::new(id).small().label(label)
}

fn status_pill(label: String) -> impl IntoElement {
    div()
        .px_2()
        .py_1()
        .rounded_sm()
        .bg(rgb(0x1d2430))
        .text_xs()
        .text_color(rgb(0x9ca8ba))
        .child(label)
}

fn composer(input: Entity<InputState>, cx: &mut Context<CodexGui>) -> impl IntoElement {
    div()
        .border_t_1()
        .border_color(cx.theme().border)
        .p_4()
        .flex()
        .items_end()
        .gap_3()
        .child(Input::new(&input).h(px(112.)).flex_1())
        .child(
            command_button("send-composer-turn", "Send")
                .primary()
                .on_click(cx.listener(|view, _, window, cx| view.send_composer_turn(window, cx))),
        )
}

fn render_message(message: &Message) -> impl IntoElement {
    match message {
        Message::User(body) => message_card("YOU", body, rgb(0x1f2937).into(), None),
        Message::Commentary(body) => message_card("COMMENTARY", body, rgb(0x1b2430).into(), None),
        Message::Assistant {
            body, state, tools, ..
        } => {
            let tool_list = tools
                .iter()
                .fold(div().flex().flex_col().gap_2(), |list, tool| {
                    list.child(render_tool_call(tool))
                });
            message_card(
                match state {
                    StreamState::Complete => "CODEX",
                    StreamState::Streaming => "CODEX IS WORKING",
                },
                body,
                rgb(0x161b23).into(),
                Some(tool_list),
            )
        }
    }
}

fn message_card(
    author: &'static str,
    body: &str,
    background: gpui::Hsla,
    child: Option<gpui::Div>,
) -> impl IntoElement {
    div()
        .rounded_md()
        .border_1()
        .border_color(rgb(0x2a303b))
        .bg(background)
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .child(div().text_xs().text_color(rgb(0x7c879a)).child(author))
        .child(div().text_sm().line_height(px(22.)).child(body.to_string()))
        .when_some(child, |this, child| this.child(child))
}

fn render_tool_call(tool: &ToolCall) -> impl IntoElement {
    let (label, color) = match tool.status {
        ToolStatus::Running => ("running", gpui::Hsla::from(rgb(0xf59e0b))),
        ToolStatus::Done => ("done", gpui::Hsla::from(rgb(0x22c55e))),
    };

    div()
        .rounded_sm()
        .border_1()
        .border_color(rgb(0x303746))
        .bg(rgb(0x111722))
        .p_3()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_sm().child(tool.name.clone()))
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(0x919bad))
                        .child(tool.detail.clone()),
                ),
        )
        .child(
            div()
                .px_2()
                .py_1()
                .rounded_sm()
                .bg(color.opacity(0.18))
                .text_color(color)
                .text_xs()
                .child(label),
        )
}
