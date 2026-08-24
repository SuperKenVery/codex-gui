mod messages;
mod tools;
mod worked_summary;

use std::{
    collections::hash_map::DefaultHasher,
    fmt,
    hash::{Hash as _, Hasher as _},
    sync::Arc,
    time::Duration,
};

use gpui::{AnyElement, App, IntoElement, SharedString, WeakEntity, Window};
use gpui::{InteractiveElement as _, StatefulInteractiveElement as _};
use gpui_component::ActiveTheme as _;

use super::view::ChatHistory;
pub(super) use tools::{ToolCallView, is_tool_item, tool_call_views, tools_done};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct BlockId(String);

impl BlockId {
    pub fn new(kind: &'static str, key: &str) -> Self {
        let mut hasher = DefaultHasher::new();
        kind.hash(&mut hasher);
        key.hash(&mut hasher);
        Self(format!("block-{:016x}", hasher.finish()))
    }

    pub fn from_marker(value: String) -> Self {
        Self(value)
    }

    pub fn tool_group(key: &str) -> Self {
        Self::new("tools", key)
    }
}

impl fmt::Display for BlockId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone)]
pub(super) enum HistoryBlock {
    User {
        key: String,
        turn_id: String,
        previous_turn_id: Option<String>,
        body: SharedString,
    },
    AssistantHeader {
        key: String,
        label: &'static str,
    },
    ToolGroup {
        key: String,
        tools: Arc<[ToolCallView]>,
        expanded: bool,
        collapsible: bool,
        active_tail: bool,
    },
    WorkedSummary {
        turn_id: String,
        duration: Duration,
        expanded: bool,
    },
}

impl HistoryBlock {
    pub fn id(&self) -> BlockId {
        match self {
            Self::User { key, .. } => BlockId::new("user", key),
            Self::AssistantHeader { key, .. } => BlockId::new("assistant-header", key),
            Self::ToolGroup { key, .. } => BlockId::tool_group(key),
            Self::WorkedSummary { turn_id, .. } => BlockId::new("worked-summary", turn_id),
        }
    }
}

pub(super) fn render(
    history: &WeakEntity<ChatHistory>,
    block: HistoryBlock,
    _window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    match block {
        HistoryBlock::User {
            key,
            turn_id,
            previous_turn_id,
            body,
        } => {
            let edit_history = history.clone();
            let edit_turn_id = turn_id.clone();
            let edit_body = body.clone();
            let fork_history = history.clone();
            messages::render_user(
                &key,
                body,
                cx.theme(),
                move |_, _, cx| {
                    cx.stop_propagation();
                    let turn_id = edit_turn_id.clone();
                    let previous_turn_id = previous_turn_id.clone();
                    let body = edit_body.to_string();
                    let _ = edit_history.update(cx, |history, cx| {
                        history.edit_user_message(turn_id, previous_turn_id, body, cx)
                    });
                },
                move |_, _, cx| {
                    cx.stop_propagation();
                    let turn_id = turn_id.clone();
                    let _ = fork_history
                        .update(cx, |history, cx| history.fork_user_message(turn_id, cx));
                },
            )
            .into_any_element()
        }
        HistoryBlock::AssistantHeader { label, .. } => {
            messages::render_assistant_header(label, cx.theme()).into_any_element()
        }
        HistoryBlock::ToolGroup {
            key,
            tools,
            expanded,
            collapsible,
            active_tail,
        } => {
            let history = history.clone();
            tools::render_group(
                &tools,
                collapsible,
                active_tail,
                expanded,
                cx.theme(),
                move |_, _, cx| {
                    let key = key.clone();
                    let _ = history.update(cx, |history, cx| history.toggle_tools(&key, cx));
                },
            )
            .into_any_element()
        }
        HistoryBlock::WorkedSummary {
            turn_id,
            duration,
            expanded,
        } => {
            let history = history.clone();
            let element_id = format!("worked-summary-{turn_id}");
            worked_summary::render(duration, cx.theme(), expanded)
                .id(element_id)
                .on_click(move |_, _, cx| {
                    let turn_id = turn_id.clone();
                    let _ = history.update(cx, |history, cx| history.toggle_turn(&turn_id, cx));
                })
                .into_any_element()
        }
    }
}
