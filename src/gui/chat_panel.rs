use crate::app::CodexGui;
use crate::gui::{
    ApprovalReviewerMode, BridgeState, ChatHistory, GuiState, UiState, widgets::status_pill,
};
use gpui::{
    Context, Entity, IntoElement, MouseButton, ParentElement, Render, Styled, Subscription,
    WeakEntity, Window, WindowControlArea, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, IconName, Selectable as _, Side, Sizable as _,
    button::{Button, ButtonVariants as _},
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
    scroll::ScrollableElement,
};

pub struct ChatPanel {
    parent: WeakEntity<CodexGui>,
    state: Entity<GuiState>,
    ui_state: Entity<UiState>,
    bridge_state: Entity<BridgeState>,
    history: Entity<ChatHistory>,
    composer_input: Entity<InputState>,
    project_path_input: Entity<InputState>,
    should_move_window: bool,
    _subscriptions: Vec<Subscription>,
}

impl ChatPanel {
    pub fn new(
        parent: WeakEntity<CodexGui>,
        state: Entity<GuiState>,
        ui_state: Entity<UiState>,
        bridge_state: Entity<BridgeState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let history = cx.new(|cx| ChatHistory::new(state.clone(), cx));
        let composer_input = cx.new(|cx| {
            InputState::new(window, cx)
                .auto_grow(1, 5)
                .submit_on_enter(true)
                .placeholder("Do anything")
        });
        let project_path_input = cx.new(|cx| {
            InputState::new(window, cx)
                .submit_on_enter(true)
                .placeholder("/path/to/project")
        });
        let subscriptions = vec![
            cx.observe(&state, |_, _, cx| cx.notify()),
            cx.observe(&ui_state, |_, _, cx| cx.notify()),
            cx.observe(&bridge_state, |_, _, cx| cx.notify()),
            cx.subscribe_in(&composer_input, window, |view, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { shift: false, .. }) {
                    if view.active_chat_turn_running(cx) {
                        view.steer_composer_turn(window, cx);
                    } else {
                        view.send_composer_turn(window, cx);
                    }
                }
            }),
            cx.subscribe_in(&project_path_input, window, |view, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { shift: false, .. }) {
                    view.add_project_from_input(window, cx);
                }
            }),
        ];

        Self {
            parent,
            state,
            ui_state,
            bridge_state,
            history,
            composer_input,
            project_path_input,
            should_move_window: false,
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
            let _ = parent.update(cx, |parent, cx| parent.submit_turn_text(text, cx));
        });
    }

    fn steer_composer_turn(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
            let _ = parent.update(cx, |parent, cx| parent.steer_turn_text(text, cx));
        });
    }

    fn stop_active_turn(&mut self, cx: &mut Context<Self>) {
        let parent = self.parent.clone();
        cx.defer(move |cx| {
            let _ = parent.update(cx, |parent, cx| parent.stop_active_turn(cx));
        });
    }

    fn active_chat_turn_running(&self, cx: &mut Context<Self>) -> bool {
        let (project, active_chat) = {
            let state = self.state.read(cx);
            (state.active_project(), state.active_chat)
        };
        let active_thread_id = project.and_then(|project| {
            project
                .read(cx)
                .chats
                .get(active_chat)
                .map(|chat| chat.read(cx).id.clone())
        });
        let Some(active_thread_id) = active_thread_id else {
            return false;
        };
        self.ui_state
            .read(cx)
            .active_turn
            .as_ref()
            .is_some_and(|active_turn| active_turn.thread_id == active_thread_id)
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

    fn select_project(&mut self, index: usize, cx: &mut Context<Self>) {
        let parent = self.parent.clone();
        cx.defer(move |cx| {
            let _ = parent.update(cx, |parent, cx| parent.select_project(index, cx));
        });
    }

    fn add_project_from_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let path = self.project_path_input.update(cx, |input, cx| {
            let path = input.value().trim().to_string();
            if !path.is_empty() {
                input.set_value("", window, cx);
            }
            path
        });
        if path.is_empty() {
            return;
        }
        let parent = self.parent.clone();
        cx.defer(move |cx| {
            let _ = parent.update(cx, |parent, cx| parent.add_project(path, cx));
        });
    }

    fn composer_surface(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
        let turn_running = self.active_chat_turn_running(cx);

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
                    .px_2()
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
                                                            let _ =
                                                                parent.update(cx, |parent, cx| {
                                                                    parent.set_model(id, cx)
                                                                });
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
                                                            let _ =
                                                                parent.update(cx, |parent, cx| {
                                                                    parent.set_permission_profile(
                                                                        id, cx,
                                                                    )
                                                                });
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
                                                            settings.approvals_reviewer == reviewer,
                                                        )
                                                        .on_click(move |_, _, cx| {
                                                            let _ =
                                                                parent.update(cx, |parent, cx| {
                                                                    parent.set_approvals_reviewer(
                                                                        reviewer, cx,
                                                                    )
                                                                });
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
                                            let mut menu = menu.min_w(190.).check_side(Side::Left);
                                            for effort in &effort_options {
                                                let value = effort.clone();
                                                let parent = parent.clone();
                                                menu = menu.item(
                                                    PopupMenuItem::new(title_case(effort))
                                                        .checked(*effort == selected_effort)
                                                        .on_click(move |_, _, cx| {
                                                            let value = value.clone();
                                                            let _ =
                                                                parent.update(cx, |parent, cx| {
                                                                    parent.set_effort(value, cx)
                                                                });
                                                        }),
                                                );
                                            }
                                            menu
                                        }
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .when(turn_running, |actions| {
                                actions.child(
                                    Button::new("steer-composer-turn")
                                        .small()
                                        .primary()
                                        .rounded(px(999.))
                                        .icon(IconName::ArrowUp)
                                        .tooltip("Steer")
                                        .on_click(cx.listener(|view, _, window, cx| {
                                            view.steer_composer_turn(window, cx);
                                        })),
                                )
                            })
                            .child(
                                Button::new("send-or-stop-composer-turn")
                                    .small()
                                    .when(!turn_running, |button| button.primary())
                                    .when(turn_running, |button| button.danger())
                                    .rounded(px(999.))
                                    .icon(if turn_running {
                                        IconName::Close
                                    } else {
                                        IconName::ArrowUp
                                    })
                                    .tooltip(if turn_running { "Stop" } else { "Send" })
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        if view.active_chat_turn_running(cx) {
                                            view.stop_active_turn(cx);
                                        } else {
                                            view.send_composer_turn(window, cx);
                                        }
                                    })),
                            ),
                    ),
            )
    }

    fn composer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w_full()
            .px_5()
            .pb_5()
            .flex()
            .justify_center()
            .child(self.composer_surface(cx))
    }

    fn new_chat_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (projects, active_project, active_project_name) = {
            let state = self.state.read(cx);
            let active_project_name = state
                .active_project()
                .map(|project| project.read(cx).name.to_string())
                .unwrap_or_else(|| "this project".into());
            (
                state.projects.clone(),
                state.active_project,
                active_project_name,
            )
        };

        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .px_5()
            .pb_20()
            .gap_7()
            .child(
                div()
                    .max_w(px(820.))
                    .w_full()
                    .text_center()
                    .text_3xl()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(format!("What should we build in {active_project_name}?")),
            )
            .child(
                div()
                    .w_full()
                    .max_w(px(820.))
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(self.composer_surface(cx))
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap_2()
                            .overflow_x_scrollbar()
                            .children(projects.iter().enumerate().map(|(index, project)| {
                                let project = project.read(cx);
                                Button::new(format!("new-chat-project-{index}"))
                                    .small()
                                    .ghost()
                                    .selected(index == active_project)
                                    .icon(if index == active_project {
                                        IconName::FolderOpen
                                    } else {
                                        IconName::Folder
                                    })
                                    .label(project.name.clone())
                                    .tooltip(project.path.clone())
                                    .on_click(cx.listener(move |view, _, _, cx| {
                                        view.select_project(index, cx)
                                    }))
                            })),
                    )
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .rounded_lg()
                                    .border_1()
                                    .border_color(cx.theme().border)
                                    .bg(cx.theme().background)
                                    .child(
                                        Input::new(&self.project_path_input)
                                            .appearance(false)
                                            .h(px(34.))
                                            .w_full(),
                                    ),
                            )
                            .child(
                                Button::new("add-project")
                                    .small()
                                    .ghost()
                                    .icon(IconName::Plus)
                                    .label("Add project")
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.add_project_from_input(window, cx)
                                    })),
                            ),
                    ),
            )
    }
}

impl Render for ChatPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let new_chat_open =
            self.state.read(cx).active_project().is_some() && self.ui_state.read(cx).new_chat_open;
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
                    .window_control_area(WindowControlArea::Drag)
                    .on_mouse_down_out(cx.listener(|view, _, _, _| {
                        view.should_move_window = false;
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, _, _, _| {
                            view.should_move_window = true;
                        }),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|view, _, _, _| {
                            view.should_move_window = false;
                        }),
                    )
                    .on_mouse_move(cx.listener(|view, _, window, _| {
                        if view.should_move_window {
                            view.should_move_window = false;
                            window.start_window_move();
                        }
                    }))
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
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation();
                                    })
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
            .when(new_chat_open, |this| this.child(self.new_chat_page(cx)))
            .when(!new_chat_open, |this| {
                this.child(
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
            })
    }
}

fn title_case(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
