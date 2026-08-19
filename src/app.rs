use crate::bridge::{AppServerBridge, BridgeEvent};
use crate::gui::{ChatPanel, GuiState, ProjectState, SideChat, Sidebar, UiState};
use crate::workspace::workspace_path;
mod actions;
mod effects;
mod event_handler;
mod results;
mod thread_mapping;

use gpui::{
    Context, Entity, IntoElement, ParentElement, Render, StyleRefinement, Styled, Subscription,
    Task, Window, div, prelude::*, px, transparent_black,
};
use gpui_component::ActiveTheme as _;
use tokio::sync::mpsc::UnboundedReceiver;

pub struct CodexGui {
    state: Entity<GuiState>,
    ui_state: Entity<UiState>,
    bridge: AppServerBridge,
    pending_turn_text: Option<String>,
    sidebar: Entity<Sidebar>,
    chat_panel: Entity<ChatPanel>,
    side_chat: Entity<SideChat>,
    _bridge_task: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl CodexGui {
    pub fn new(
        bridge: AppServerBridge,
        mut bridge_rx: UnboundedReceiver<BridgeEvent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let initial_projects = initial_projects(cx);
        let state = cx.new(|_| GuiState::new(initial_projects));
        let ui_state = cx.new(|_| UiState::new());
        let parent = cx.entity().downgrade();
        let sidebar = cx.new(|cx| Sidebar::new(parent.clone(), state.clone(), cx));
        let chat_panel = cx
            .new(|cx| ChatPanel::new(parent.clone(), state.clone(), ui_state.clone(), window, cx));
        let side_chat = cx.new(|cx| SideChat::new(state.clone(), cx));

        let bridge_task = cx.spawn(async move |this, cx| {
            while let Some(event) = bridge_rx.recv().await {
                if this
                    .update(cx, |view, cx| view.apply_bridge_event(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        });

        let initialize_bridge = bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = initialize_bridge.wait_until_ready().await;
            let _ = this.update(cx, |view, cx| view.apply_initialize_result(result, cx));
        })
        .detach();

        let shutdown_subscription = cx.on_app_quit(|view, _cx| {
            let bridge = view.bridge.clone();
            async move { bridge.shutdown().await }
        });

        Self {
            state,
            ui_state,
            bridge,
            pending_turn_text: None,
            sidebar,
            chat_panel,
            side_chat,
            _bridge_task: bridge_task,
            _subscriptions: vec![shutdown_subscription],
        }
    }
}

fn initial_projects(cx: &mut Context<CodexGui>) -> Vec<Entity<ProjectState>> {
    let path = workspace_path();
    let name = thread_mapping::project_name_from_path(&path);
    vec![cx.new(|_| ProjectState::new(name.into(), path.into(), Vec::new()))]
}

impl Render for CodexGui {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                        .child(
                            self.sidebar.clone().cached(
                                StyleRefinement::default().w(px(286.)).h_full().flex_none(),
                            ),
                        )
                        .child(self.chat_panel.clone())
                        .when(side_chat_open, |this| this.child(self.side_chat.clone())),
                ),
            )
    }
}
