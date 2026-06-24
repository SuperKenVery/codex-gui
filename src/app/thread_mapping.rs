use super::CodexGui;
use crate::gui::{ChatState, HistoryEntryKind, HistoryKey, MessageState, StreamState};
use codex_app_server_protocol::{Thread, ThreadItem, ThreadStatus, UserInput};
use gpui::{AppContext, Context, Entity};
use std::path::Path;

pub(super) fn chat_entity_from_thread(
    thread: Thread,
    cx: &mut Context<CodexGui>,
) -> Entity<ChatState> {
    let title = thread_title(thread.name.as_deref(), &thread.preview);
    let subtitle = format!(
        "{} - {}",
        thread_status_label(&thread.status),
        thread.cwd.display()
    );
    let mut messages = Vec::new();
    for turn in &thread.turns {
        for item in &turn.items {
            append_thread_item(&mut messages, item.clone(), cx);
        }
    }
    cx.new(|_| ChatState::from_thread(thread, title.into(), subtitle.into(), messages))
}

pub(super) fn thread_title(name: Option<&str>, preview: &str) -> String {
    name.filter(|name| !name.trim().is_empty())
        .or_else(|| {
            let preview = preview.trim();
            (!preview.is_empty()).then_some(preview)
        })
        .unwrap_or("Untitled Codex thread")
        .to_string()
}

pub(super) fn empty_chat_entity(cx: &mut Context<CodexGui>) -> Entity<ChatState> {
    cx.new(|cx| {
        ChatState::new(
            "empty".into(),
            "No Codex threads".into(),
            "Click New to start one in this workspace".into(),
            vec![cx.new(|cx| {
                MessageState::notice(
                    "No persisted Codex threads were returned for this workspace.".into(),
                    cx,
                )
            })],
        )
    })
}

pub(super) fn project_name_from_path(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
        .to_string()
}

pub(super) fn append_thread_item(
    messages: &mut Vec<Entity<MessageState>>,
    item: ThreadItem,
    cx: &mut Context<CodexGui>,
) {
    match item {
        ThreadItem::UserMessage { id, content, .. } => {
            let text = user_input_text(&content);
            if !text.is_empty() {
                messages.push(cx.new(|cx| {
                    MessageState::item(
                        HistoryKey::Item(id),
                        HistoryEntryKind::User,
                        None,
                        StreamState::Complete,
                        cx,
                    )
                }));
            }
        }
        ThreadItem::AgentMessage { id, text, .. } => {
            messages.push(cx.new(|cx| {
                MessageState::item(
                    HistoryKey::Item(id.clone()),
                    HistoryEntryKind::Assistant,
                    Some(text),
                    StreamState::Complete,
                    cx,
                )
            }));
        }
        ThreadItem::CommandExecution { id, .. } => {
            push_tool_to_messages(messages, id, cx);
        }
        ThreadItem::FileChange { id, .. } => {
            push_tool_to_messages(messages, id, cx);
        }
        ThreadItem::McpToolCall { id, .. } => {
            push_tool_to_messages(messages, id, cx);
        }
        ThreadItem::DynamicToolCall { id, .. } => {
            push_tool_to_messages(messages, id, cx);
        }
        _ => {}
    }
}

pub(super) fn push_tool_to_messages(
    messages: &mut Vec<Entity<MessageState>>,
    tool_id: String,
    cx: &mut Context<CodexGui>,
) {
    for message in messages.iter().rev() {
        let is_assistant = matches!(message.read(cx).kind, HistoryEntryKind::Assistant);
        if is_assistant {
            return;
        }
    }

    messages.push(cx.new(|cx| {
        MessageState::item(
            HistoryKey::ToolGroup(tool_id),
            HistoryEntryKind::ToolGroup,
            None,
            StreamState::Complete,
            cx,
        )
    }));
}

pub(super) fn thread_status_label(status: &ThreadStatus) -> &'static str {
    match status {
        ThreadStatus::NotLoaded => "not loaded",
        ThreadStatus::Idle => "idle",
        ThreadStatus::SystemError => "system error",
        ThreadStatus::Active { .. } => "active",
    }
}

pub(super) fn user_input_text(content: &[UserInput]) -> String {
    content
        .iter()
        .filter_map(|input| match input {
            UserInput::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn should_start_thread_for_turn(
    new_chat_open: bool,
    active_thread_id: Option<&str>,
) -> bool {
    new_chat_open || active_thread_id.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_chat_turn_starts_thread_even_when_an_active_thread_exists() {
        assert!(should_start_thread_for_turn(
            true,
            Some("existing-thread-id")
        ));
    }

    #[test]
    fn existing_chat_turn_reuses_active_thread() {
        assert!(!should_start_thread_for_turn(
            false,
            Some("existing-thread-id")
        ));
    }

    #[test]
    fn missing_active_chat_starts_thread() {
        assert!(should_start_thread_for_turn(false, None));
    }

    #[test]
    fn thread_title_prefers_name() {
        assert_eq!(
            thread_title(Some("Saved title"), "First prompt"),
            "Saved title"
        );
    }

    #[test]
    fn thread_title_falls_back_to_preview() {
        assert_eq!(thread_title(None, "  First prompt  "), "First prompt");
    }

    #[test]
    fn thread_title_uses_default_when_name_and_preview_are_empty() {
        assert_eq!(thread_title(Some("   "), " "), "Untitled Codex thread");
    }
}
