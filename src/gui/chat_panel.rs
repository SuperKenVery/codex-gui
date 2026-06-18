use crate::app::CodexGui;
use crate::gui::{ApprovalReviewerMode, BridgeState, ChatHistory, GuiState, widgets::status_pill};
use gpui::{
    Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, WeakEntity, Window,
    div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, IconName, Side, Sizable as _,
    button::{Button, ButtonVariants as _},
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
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

    fn fork_chat(&mut self, cx: &mut Context<Self>) {
        let parent = self.parent.clone();
        cx.defer(move |cx| {
            let _ = parent.update(cx, |parent, cx| parent.fork_chat(cx));
        });
    }

    fn toggle_side_chat(&mut self, cx: &mut Context<Self>) {
        let parent = self.parent.clone();
        cx.defer(move |cx| {
            let _ = parent.update(cx, |parent, cx| parent.toggle_side_chat(cx));
        });
    }

    fn composer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (settings, models, permission_profiles) = {
            let state = self.state.read(cx);
            (
                state.chat_settings.clone(),
                state.available_models.clone(),
                state.permission_profiles.clone(),
            )
        };
        let model_label =
            if let Some(model) = models.iter().find(|model| model.id == settings.model) {
                model.display_name.clone()
            } else {
                settings.model.clone()
            };
        let effort_options = models
            .iter()
            .find(|model| model.id == settings.model)
            .map(|model| model.supported_efforts.clone())
            .filter(|efforts| !efforts.is_empty())
            .unwrap_or_else(|| {
                vec![
                    "none".into(),
                    "minimal".into(),
                    "low".into(),
                    "medium".into(),
                    "high".into(),
                    "xhigh".into(),
                ]
            });

        div().w_full().px_5().pb_5().flex().justify_center().child(
            div()
                .w_full()
                .max_w(px(820.))
                .rounded_3xl()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().background)
                .shadow_sm()
                .p_2()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    Input::new(&self.composer_input)
                        .appearance(false)
                        .h(px(92.))
                        .w_full(),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    Button::new("composer-model")
                                        .small()
                                        .ghost()
                                        .icon(IconName::Cpu)
                                        .label(model_label)
                                        .tooltip("Model settings")
                                        .dropdown_menu({
                                            let parent = self.parent.clone();
                                            let models = models.clone();
                                            let selected_model = settings.model.clone();
                                            move |menu, _, _| {
                                                let mut menu = menu
                                                    .min_w(260.)
                                                    .max_h(px(360.))
                                                    .scrollable(true)
                                                    .check_side(Side::Left);
                                                if models.is_empty() {
                                                    return menu.item(
                                                        PopupMenuItem::new("Loading models")
                                                            .disabled(true),
                                                    );
                                                }
                                                for model in &models {
                                                    let id = model.id.clone();
                                                    let label = model.display_name.clone();
                                                    let parent = parent.clone();
                                                    menu = menu.item(
                                                        PopupMenuItem::new(label)
                                                            .checked(model.id == selected_model)
                                                            .on_click(move |_, _, cx| {
                                                                let id = id.clone();
                                                                let _ = parent.update(
                                                                    cx,
                                                                    |parent, cx| {
                                                                        parent.set_model(id, cx)
                                                                    },
                                                                );
                                                            }),
                                                    );
                                                }
                                                menu
                                            }
                                        }),
                                )
                                .child(
                                    Button::new("composer-permissions")
                                        .small()
                                        .ghost()
                                        .icon(IconName::Check)
                                        .label(settings.approvals_reviewer.label())
                                        .tooltip("Permission settings")
                                        .dropdown_menu({
                                            let parent = self.parent.clone();
                                            let settings = settings.clone();
                                            let permission_profiles = permission_profiles.clone();
                                            move |menu, _, _| {
                                                let mut menu = menu
                                                    .min_w(240.)
                                                    .check_side(Side::Left)
                                                    .item(PopupMenuItem::label("Permissions"));
                                                for profile in &permission_profiles {
                                                    let id = profile.id.clone();
                                                    let parent = parent.clone();
                                                    menu = menu.item(
                                                        PopupMenuItem::new(profile.label.clone())
                                                            .checked(
                                                                settings.permission_profile
                                                                    == profile.id,
                                                            )
                                                            .on_click(move |_, _, cx| {
                                                                let id = id.clone();
                                                                let _ = parent.update(
                                                                    cx,
                                                                    |parent, cx| {
                                                                        parent
                                                                            .set_permission_profile(
                                                                                id, cx,
                                                                            )
                                                                    },
                                                                );
                                                            }),
                                                    );
                                                }
                                                menu = menu
                                                    .separator()
                                                    .item(PopupMenuItem::label("Approvals"));
                                                for reviewer in [
                                                    ApprovalReviewerMode::User,
                                                    ApprovalReviewerMode::AutoReview,
                                                ] {
                                                    let parent = parent.clone();
                                                    menu = menu.item(
                                                        PopupMenuItem::new(reviewer.label())
                                                            .checked(
                                                                settings.approvals_reviewer
                                                                    == reviewer,
                                                            )
                                                            .on_click(move |_, _, cx| {
                                                                let _ = parent.update(
                                                                    cx,
                                                                    |parent, cx| {
                                                                        parent
                                                                            .set_approvals_reviewer(
                                                                                reviewer, cx,
                                                                            )
                                                                    },
                                                                );
                                                            }),
                                                    );
                                                }
                                                menu
                                            }
                                        }),
                                )
                                .child(
                                    Button::new("composer-effort")
                                        .small()
                                        .ghost()
                                        .icon(IconName::LoaderCircle)
                                        .label(format!("Effort {}", title_case(&settings.effort)))
                                        .tooltip("Thinking effort settings")
                                        .dropdown_menu({
                                            let parent = self.parent.clone();
                                            let selected_effort = settings.effort.clone();
                                            move |menu, _, _| {
                                                let mut menu =
                                                    menu.min_w(190.).check_side(Side::Left);
                                                for effort in &effort_options {
                                                    let value = effort.clone();
                                                    let parent = parent.clone();
                                                    menu = menu.item(
                                                        PopupMenuItem::new(title_case(effort))
                                                            .checked(*effort == selected_effort)
                                                            .on_click(move |_, _, cx| {
                                                                let value = value.clone();
                                                                let _ = parent.update(
                                                                    cx,
                                                                    |parent, cx| {
                                                                        parent.set_effort(value, cx)
                                                                    },
                                                                );
                                                            }),
                                                    );
                                                }
                                                menu
                                            }
                                        }),
                                ),
                        )
                        .child(
                            Button::new("send-composer-turn")
                                .small()
                                .primary()
                                .icon(IconName::ArrowUp)
                                .tooltip("Send")
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.send_composer_turn(window, cx)
                                })),
                        ),
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
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .px_5()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(820.))
                            .mx_auto()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
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
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(status_pill(bridge_status, cx.theme()))
                                    .child(
                                        Button::new("fork-chat")
                                            .small()
                                            .ghost()
                                            .icon(IconName::Copy)
                                            .tooltip("Fork chat")
                                            .on_click(
                                                cx.listener(|view, _, _, cx| view.fork_chat(cx)),
                                            ),
                                    )
                                    .child(
                                        Button::new("toggle-side-chat")
                                            .small()
                                            .ghost()
                                            .icon(IconName::PanelRightOpen)
                                            .tooltip("Open side chat")
                                            .on_click(cx.listener(|view, _, _, cx| {
                                                view.toggle_side_chat(cx)
                                            })),
                                    ),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    .px_5()
                    .py_4()
                    .child(
                        div()
                            .size_full()
                            .max_w(px(820.))
                            .mx_auto()
                            .child(self.history.clone()),
                    ),
            )
            .child(self.composer(cx))
    }
}

fn title_case(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
