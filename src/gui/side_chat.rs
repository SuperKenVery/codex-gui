use crate::gui::{GuiState, Message, widgets::render_message};
use gpui::{
    Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window, div, px, rgb,
};
use gpui_component::ActiveTheme as _;

pub struct SideChat {
    state: Entity<GuiState>,
    _subscriptions: Vec<Subscription>,
}

impl SideChat {
    pub fn new(state: Entity<GuiState>, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![cx.observe(&state, |_, _, cx| cx.notify())];
        Self {
            state,
            _subscriptions: subscriptions,
        }
    }
}

impl Render for SideChat {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let thread_title = {
            let (project, active_chat) = {
                let state = self.state.read(cx);
                (state.active_project(), state.active_chat)
            };
            let chat = project.and_then(|project| {
                let chats = project.read(cx).chats.clone();
                chats.get(active_chat).cloned()
            });
            chat.map(|chat| chat.read(cx).title.to_string())
                .unwrap_or_else(|| "No thread".into())
        };

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
                            .child(format!("Temporary view of {thread_title}.")),
                    )
                    .child(render_message(&Message::Commentary(
                        "Side chats remain a UI-only view until promoted through thread/fork."
                            .into(),
                    ))),
            )
    }
}
