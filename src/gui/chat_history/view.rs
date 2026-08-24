use std::{
    collections::HashSet,
    sync::{Arc, RwLock},
};

use gpui::{
    Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window, div,
    prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _,
    clipboard::Clipboard,
    text::{MarkdownExtensions, TextView, TextViewState, TextViewStyle},
};

use crate::gui::{ChatState, GuiState, widgets::render_notice};

use super::{
    math::MathPlugin,
    projection::build_transcript,
    transcript::{TranscriptBlockStore, TranscriptPlugin, TranscriptSnapshot},
};

pub struct ChatHistory {
    state: Entity<GuiState>,
    active_chat: Option<Entity<ChatState>>,
    _state_subscription: Subscription,
    chat_subscription: Option<Subscription>,
    expanded_turns: HashSet<String>,
    expanded_tool_groups: HashSet<String>,
    transcript: Entity<TextViewState>,
    transcript_extensions: MarkdownExtensions,
    transcript_blocks: TranscriptBlockStore,
    transcript_markdown: String,
    transcript_chat_id: Option<String>,
}

impl ChatHistory {
    pub fn new(state: Entity<GuiState>, cx: &mut Context<Self>) -> Self {
        let active_chat = active_chat_entity(&state, cx);
        let chat_subscription = subscribe_to_chat(active_chat.as_ref(), cx);
        let state_subscription = cx.observe(&state, |history, _, cx| {
            history.update_active_chat_subscription(cx);
            cx.notify();
        });
        let transcript = cx.new(|cx| TextViewState::markdown("", cx));
        let transcript_blocks = Arc::new(RwLock::new(Default::default()));
        let transcript_extensions =
            TranscriptPlugin::new(cx.entity().downgrade(), transcript_blocks.clone())
                .extensions()
                .plugin(MathPlugin::new());

        let mut history = Self {
            state,
            active_chat,
            _state_subscription: state_subscription,
            chat_subscription,
            expanded_turns: HashSet::new(),
            expanded_tool_groups: HashSet::new(),
            transcript,
            transcript_extensions,
            transcript_blocks,
            transcript_markdown: String::new(),
            transcript_chat_id: None,
        };
        history.rebuild_transcript(cx);
        history
    }

    fn update_active_chat_subscription(&mut self, cx: &mut Context<Self>) {
        let active_chat = active_chat_entity(&self.state, cx);
        if self.active_chat == active_chat {
            return;
        }

        self.chat_subscription = subscribe_to_chat(active_chat.as_ref(), cx);
        self.active_chat = active_chat;
        self.expanded_turns.clear();
        self.expanded_tool_groups.clear();
        self.transcript = cx.new(|cx| TextViewState::markdown("", cx));
        self.transcript_markdown.clear();
        self.transcript_chat_id = None;
        self.rebuild_transcript(cx);
    }

    pub(super) fn toggle_turn(&mut self, turn_id: &str, cx: &mut Context<Self>) {
        if !self.expanded_turns.remove(turn_id) {
            self.expanded_turns.insert(turn_id.to_string());
        }
        self.rebuild_transcript(cx);
        cx.notify();
    }

    pub(super) fn toggle_tools(&mut self, group_id: &str, cx: &mut Context<Self>) {
        if !self.expanded_tool_groups.remove(group_id) {
            self.expanded_tool_groups.insert(group_id.to_string());
        }
        self.rebuild_transcript(cx);
        cx.notify();
    }

    fn rebuild_transcript(&mut self, cx: &mut Context<Self>) {
        let Some(chat) = self.active_chat.as_ref() else {
            self.sync_transcript(None, TranscriptSnapshot::new(), cx);
            return;
        };
        let chat_id = chat.read(cx).id.clone();
        let snapshot = build_transcript(
            chat.read(cx),
            &self.expanded_turns,
            &self.expanded_tool_groups,
        );
        self.sync_transcript(Some(chat_id), snapshot, cx);
    }

    fn sync_transcript(
        &mut self,
        chat_id: Option<String>,
        snapshot: TranscriptSnapshot,
        cx: &mut Context<Self>,
    ) {
        if let Ok(mut blocks) = self.transcript_blocks.write() {
            *blocks = snapshot.blocks;
        }

        let same_chat = self.transcript_chat_id == chat_id;
        let old_markdown = std::mem::replace(&mut self.transcript_markdown, snapshot.markdown);
        self.transcript_chat_id = chat_id;

        if same_chat && old_markdown == self.transcript_markdown {
            return;
        }

        let appended = if same_chat && !old_markdown.is_empty() {
            self.transcript_markdown
                .strip_prefix(&old_markdown)
                .map(str::to_string)
        } else {
            None
        };
        let markdown = self.transcript_markdown.clone();
        self.transcript.update(cx, |state, cx| {
            if let Some(delta) = appended {
                state.push_str(&delta, cx);
            } else {
                state.set_text(&markdown, cx);
            }
        });
    }
}

impl Render for ChatHistory {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.active_chat.is_none() {
            return div()
                .id("message-list")
                .flex()
                .flex_col()
                .size_full()
                .min_w_0()
                .gap_3()
                .overflow_x_hidden()
                .overflow_y_scroll()
                .child(render_notice(
                    "Loading Codex threads from the app server.",
                    cx.theme(),
                ))
                .into_any_element();
        }

        div()
            .id("message-list")
            .size_full()
            .min_w_0()
            .overflow_hidden()
            .child(
                TextView::new(&self.transcript)
                    .markdown_extensions(self.transcript_extensions.clone())
                    .code_block_actions(|code_block, _window, _cx| {
                        Clipboard::new("copy-code-block")
                            .value(code_block.code())
                            .tooltip("Copy code")
                    })
                    .selectable(true)
                    .scrollable(true)
                    .content_max_width(px(820.))
                    .size_full()
                    .min_w_0()
                    .text_base()
                    .line_height(px(30.))
                    .style(TextViewStyle {
                        ..Default::default()
                    })
                    .text_color(cx.theme().foreground),
            )
            .into_any_element()
    }
}

fn subscribe_to_chat(
    chat: Option<&Entity<ChatState>>,
    cx: &mut Context<ChatHistory>,
) -> Option<Subscription> {
    chat.map(|chat| {
        cx.observe(chat, |history, _, cx| {
            history.rebuild_transcript(cx);
            cx.notify();
        })
    })
}

fn active_chat_entity(
    state: &Entity<GuiState>,
    cx: &mut Context<ChatHistory>,
) -> Option<Entity<ChatState>> {
    state.read(cx).active_chat_entity(cx)
}
