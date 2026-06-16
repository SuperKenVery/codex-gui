#![cfg_attr(target_family = "wasm", no_main)]

use gpui::{
    App, Bounds, Context, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Window, WindowBounds, WindowOptions, div, prelude::*, px,
    rgb, rgba, size,
};
use gpui_platform::application;

#[derive(Clone)]
struct Project {
    name: SharedString,
    path: SharedString,
    chats: Vec<Chat>,
}

#[derive(Clone)]
struct Chat {
    title: SharedString,
    subtitle: SharedString,
    messages: Vec<Message>,
}

#[derive(Clone)]
enum Message {
    User(&'static str),
    Assistant {
        body: &'static str,
        state: StreamState,
        tools: Vec<ToolCall>,
    },
    Commentary(&'static str),
}

#[derive(Clone, Copy)]
enum StreamState {
    Complete,
    Streaming,
}

#[derive(Clone)]
struct ToolCall {
    name: &'static str,
    status: ToolStatus,
    detail: &'static str,
}

#[derive(Clone, Copy)]
enum ToolStatus {
    Running,
    Done,
}

struct CodexGui {
    projects: Vec<Project>,
    active_project: usize,
    active_chat: usize,
    side_chat_open: bool,
}

impl CodexGui {
    fn new() -> Self {
        Self {
            projects: sample_projects(),
            active_project: 0,
            active_chat: 0,
            side_chat_open: true,
        }
    }

    fn active_project(&self) -> &Project {
        &self.projects[self.active_project]
    }

    fn active_chat(&self) -> &Chat {
        &self.active_project().chats[self.active_chat]
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
        let mut forked = self.active_chat().clone();
        forked.title = format!("{} fork", forked.title).into();
        forked.subtitle = "Draft fork in this workspace".into();
        self.projects[self.active_project].chats.insert(0, forked);
        self.active_chat = 0;
        cx.notify();
    }

    fn toggle_side_chat(&mut self, cx: &mut Context<Self>) {
        self.side_chat_open = !self.side_chat_open;
        cx.notify();
    }
}

impl Render for CodexGui {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(rgb(0x111318))
            .text_color(rgb(0xe7e9ee))
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
        let project_items = self.projects.iter().enumerate().fold(
            div().flex().flex_col().gap_1(),
            |list, (index, project)| {
                let selected = index == self.active_project;
                list.child(
                    button(project.name.clone())
                        .id(format!("project-{index}"))
                        .when(selected, |this| this.bg(rgb(0x2a2f3a)))
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
            },
        );

        let chat_items = self.active_project().chats.iter().enumerate().fold(
            div().flex().flex_col().gap_1(),
            |list, (index, chat)| {
                let selected = index == self.active_chat;
                list.child(
                    button(chat.title.clone())
                        .id(format!("chat-{index}"))
                        .when(selected, |this| {
                            this.bg(rgb(0x283344)).border_color(rgb(0x4f80ff))
                        })
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
            .border_color(rgb(0x252933))
            .bg(rgb(0x171a21))
            .p_3()
            .gap_4()
            .child(section_label("Projects"))
            .child(project_items)
            .child(section_label("Chats"))
            .child(chat_items)
            .child(
                div()
                    .mt_auto()
                    .flex()
                    .gap_2()
                    .child(
                        command_button("Fork")
                            .id("fork-chat")
                            .on_click(cx.listener(|view, _, _, cx| view.fork_chat(cx))),
                    )
                    .child(
                        command_button("Side")
                            .id("toggle-side-chat")
                            .on_click(cx.listener(|view, _, _, cx| view.toggle_side_chat(cx))),
                    ),
            )
    }

    fn render_chat(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        let chat = self.active_chat();
        let messages = chat.messages.iter().fold(
            div()
                .id("message-list")
                .flex()
                .flex_col()
                .gap_3()
                .overflow_scroll(),
            |list, message| list.child(render_message(message)),
        );

        div()
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(0x101116))
            .child(
                div()
                    .h(px(58.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(rgb(0x252933))
                    .px_5()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(chat.title.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(0x8f98a8))
                                    .child(chat.subtitle.clone()),
                            ),
                    )
                    .child(status_pill("codex server bridge: mock")),
            )
            .child(div().flex_1().p_5().child(messages))
            .child(composer())
    }

    fn render_side_chat(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(340.))
            .h_full()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(rgb(0x252933))
            .bg(rgb(0x151820))
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
                            .child("Temporary fork of the active conversation."),
                    )
                    .child(render_message(&Message::Commentary(
                        "Side chats stay local until promoted to a normal chat.",
                    ))),
            )
            .child(composer())
    }
}

fn section_label(text: &'static str) -> impl IntoElement {
    div()
        .text_xs()
        .text_color(rgb(0x717b8f))
        .child(text.to_ascii_uppercase())
}

fn button(label: SharedString) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .w_full()
        .p_2()
        .rounded_sm()
        .border_1()
        .border_color(rgba(0xffffff0a))
        .hover(|this| this.bg(rgb(0x20242d)))
        .cursor_pointer()
        .child(div().text_sm().child(label))
}

fn command_button(label: &'static str) -> gpui::Div {
    div()
        .px_3()
        .py_2()
        .rounded_sm()
        .bg(rgb(0x2563eb))
        .hover(|this| this.bg(rgb(0x3472ff)))
        .cursor_pointer()
        .text_sm()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .child(label)
}

fn status_pill(label: &'static str) -> impl IntoElement {
    div()
        .px_2()
        .py_1()
        .rounded_sm()
        .bg(rgb(0x1d2430))
        .text_xs()
        .text_color(rgb(0x9ca8ba))
        .child(label)
}

fn composer() -> impl IntoElement {
    div().border_t_1().border_color(rgb(0x252933)).p_4().child(
        div()
            .min_h(px(82.))
            .rounded_md()
            .border_1()
            .border_color(rgb(0x343b49))
            .bg(rgb(0x171b24))
            .p_3()
            .text_color(rgb(0x98a2b5))
            .child("Ask Codex to edit, explain, test, or inspect this workspace..."),
    )
}

fn render_message(message: &Message) -> impl IntoElement {
    match message {
        Message::User(body) => message_card("YOU", body, rgb(0x1f2937).into(), None),
        Message::Commentary(body) => message_card("COMMENTARY", body, rgb(0x1b2430).into(), None),
        Message::Assistant { body, state, tools } => {
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
    body: &'static str,
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
        .child(div().text_sm().line_height(px(22.)).child(body))
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
                .child(div().text_sm().child(tool.name))
                .child(div().text_xs().text_color(rgb(0x919bad)).child(tool.detail)),
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

fn sample_projects() -> Vec<Project> {
    vec![
        Project {
            name: "codex-gui".into(),
            path: "/Users/ken/Codes/codex-gui".into(),
            chats: vec![
                Chat {
                    title: "Build desktop shell".into(),
                    subtitle: "GPUI layout and app-server bridge".into(),
                    messages: vec![
                        Message::User("Please build this."),
                        Message::Commentary(
                            "Inspecting the workspace and the local Zed GPUI sources.",
                        ),
                        Message::Assistant {
                            body: "The first pass establishes the desktop structure: project sidebar, chat list, streaming transcript, side chat, and fork action. The transport boundary is deliberately isolated so the mock stream can become the real codex app-server bridge.",
                            state: StreamState::Streaming,
                            tools: vec![
                                ToolCall {
                                    name: "rg --files",
                                    status: ToolStatus::Done,
                                    detail: "Detected greenfield repo with AGENTS.md only.",
                                },
                                ToolCall {
                                    name: "cargo check",
                                    status: ToolStatus::Running,
                                    detail: "Validating GPUI dependency shape.",
                                },
                            ],
                        },
                    ],
                },
                Chat {
                    title: "App server protocol".into(),
                    subtitle: "Queued implementation notes".into(),
                    messages: vec![Message::Assistant {
                        body: "Next transport step: model codex events as typed Rust enums and stream them into GPUI state through Context::notify.",
                        state: StreamState::Complete,
                        tools: vec![],
                    }],
                },
            ],
        },
        Project {
            name: "zed reference".into(),
            path: "/Users/ken/Projects/zed".into(),
            chats: vec![Chat {
                title: "GPUI references".into(),
                subtitle: "Examples and source patterns".into(),
                messages: vec![Message::Commentary(
                    "Use GPUI examples for rendering, window setup, scrolling, and input behavior.",
                )],
            }],
        },
    ]
}

fn run_app() {
    application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1180.), px(760.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title("codex-gui");
                cx.new(|_| CodexGui::new())
            },
        )
        .unwrap();
        cx.activate(true);
    });
}

#[cfg(not(target_family = "wasm"))]
fn main() {
    run_app();
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    gpui_platform::web_init();
    run_app();
}
