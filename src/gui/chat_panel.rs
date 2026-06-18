use crate::app::CodexGui;
use crate::gui::{
    BridgeState, ChatHistory, GuiState,
    widgets::{command_button, status_pill},
};
use gpui::{
    Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, WeakEntity, Window,
    div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _,
    button::ButtonVariants as _,
    input::{Input, InputEvent, InputState},
};

pub struct ChatPanel {
    parent: WeakEntity<CodexGui>,
    state: Entity<GuiState>,
    bridge_state: Entity<BridgeState>,
    history: Entity<ChatHistory>,
    composer_input: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl ChatPanel {
    pub fn new(
        parent: WeakEntity<CodexGui>,
        state: Entity<GuiState>,
        bridge_state: Entity<BridgeState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let history = cx.new(|cx| ChatHistory::new(state.clone(), cx));
        let composer_input = cx.new(|cx| {
            InputState::new(window, cx)
                .auto_grow(1, 5)
                .submit_on_enter(true)
                .placeholder("Ask Codex to change, explain, or inspect this project")
        });
        let subscriptions = vec![
            cx.observe(&state, |_, _, cx| cx.notify()),
            cx.observe(&bridge_state, |_, _, cx| cx.notify()),
            cx.subscribe_in(&composer_input, window, |view, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { shift: false, .. }) {
                    view.send_composer_turn(window, cx);
                }
            }),
        ];

        Self {
            parent,
            state,
            bridge_state,
            history,
            composer_input,
            _subscriptions: subscriptions,
        }
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
        let parent = self.parent.clone();
        cx.defer(move |cx| {
            let _ = parent.update(cx, |parent, cx| parent.send_turn_text(text, cx));
        });
    }

    fn composer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .border_t_1()
            .border_color(cx.theme().border)
            .p_4()
            .flex()
            .items_end()
            .gap_3()
            .child(Input::new(&self.composer_input).h(px(112.)).flex_1())
            .child(
                command_button("send-composer-turn", "Send")
                    .primary()
                    .on_click(
                        cx.listener(|view, _, window, cx| view.send_composer_turn(window, cx)),
                    ),
            )
    }
}

impl Render for ChatPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (title, subtitle, bridge_status) = {
            let (project, active_chat) = {
                let state = self.state.read(cx);
                (state.active_project(), state.active_chat)
            };
            let chat = project.and_then(|project| {
                let chats = project.read(cx).chats.clone();
                chats.get(active_chat).cloned()
            });
            let (title, subtitle) = chat
                .map(|chat| {
                    let chat = chat.read(cx);
                    (chat.title.to_string(), chat.subtitle.to_string())
                })
                .unwrap_or_else(|| ("No thread selected".into(), "Start a Codex thread".into()));
            (title, subtitle, self.bridge_state.read(cx).status.clone())
        };

        div()
            .flex_1()
            .min_w_0()
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
                    .gap_3()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .px_5()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .min_w_0()
                                    .text_lg()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .overflow_x_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .child(title),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .overflow_x_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .child(subtitle),
                            ),
                    )
                    .child(status_pill(bridge_status, cx.theme())),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    .p_5()
                    .child(self.history.clone()),
            )
            .child(self.composer(cx))
    }
}
