use crate::bridge::{AppServerBridge, BridgeEvent, start_app_server_bridge};
use crate::gui::{BridgeState, ChatPanel, GuiState, ProjectState, SideChat, Sidebar, UiState};
use crate::workspace::workspace_path;
mod actions;
mod effects;
mod event_handler;
mod results;
mod thread_mapping;

use gpui::{
    Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Task, Window, div,
    prelude::*, transparent_black,
};
use gpui_component::ActiveTheme as _;
use std::{sync::mpsc::Receiver, time::Duration};

pub struct CodexGui {
    state: Entity<GuiState>,
    ui_state: Entity<UiState>,
    bridge_state: Entity<BridgeState>,
    bridge: AppServerBridge,
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
        let (bridge, bridge_rx) = start_app_server_bridge();
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
                ui_state.clone(),
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

        let initialize_bridge = bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { initialize_bridge.initialize().await })
                .await;
            let _ = this.update(cx, |view, cx| view.apply_initialize_result(result, cx));
        })
        .detach();

        let models_bridge = bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { models_bridge.list_models().await })
                .await;
            let _ = this.update(cx, |view, cx| view.apply_models_result(result, cx));
        })
        .detach();

        let profiles_bridge = bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    profiles_bridge
                        .list_permission_profiles(workspace_path())
                        .await
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.apply_permission_profiles_result(result, cx)
            });
        })
        .detach();

        let threads_bridge = bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let cwd = workspace_path();
                    threads_bridge
                        .list_threads(cwd.clone())
                        .await
                        .map(|threads| (cwd, threads))
                })
                .await;
            let _ = this.update(cx, |view, cx| view.apply_threads_result(result, cx));
        })
        .detach();

        Self {
            state,
            ui_state,
            bridge_state,
            bridge,
            bridge_rx,
            pending_turn_text: None,
            sidebar,
            chat_panel,
            side_chat,
            _bridge_task: bridge_task,
            _subscriptions: Vec::new(),
        }
    }
}

impl Render for CodexGui {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.drain_bridge_events(cx);
        let side_chat_open = self.ui_state.read(cx).side_chat_open;

        div()
            .size_full()
            .bg(transparent_black())
            .text_color(cx.theme().foreground)
            .font_family(".SystemUIFont")
            .child(
                div().relative().size_full().child(
                    div()
                        .flex()
                        .size_full()
                        .child(self.sidebar.clone())
                        .child(self.chat_panel.clone())
                        .when(side_chat_open, |this| this.child(self.side_chat.clone())),
                ),
            )
    }
}
