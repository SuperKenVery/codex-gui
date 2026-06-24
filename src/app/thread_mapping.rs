use super::CodexGui;
use crate::gui::{
    AssistantPhase, ChatState, FileChangeKind, FileChangeSummary, Message, MessageState,
    ProjectState, StreamState, ToolCall, ToolStatus,
};
use codex_app_server_protocol::{
    CommandExecutionStatus, DynamicToolCallStatus, FileUpdateChange, McpToolCallStatus,
    PatchApplyStatus, PatchChangeKind, Thread, ThreadItem, ThreadStatus, UserInput,
};
use codex_protocol::models::MessagePhase;
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
    let thread_id = thread.id;
    let mut messages = Vec::new();
    for turn in thread.turns {
        for item in turn.items {
            append_thread_item(&mut messages, item, cx);
        }
    }
    cx.new(|_| ChatState::new(thread_id, title.into(), subtitle.into(), messages))
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
                MessageState::new(
                    Message::Notice(
                        "No persisted Codex threads were returned for this workspace.".into(),
                    ),
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
        ThreadItem::UserMessage { content, .. } => {
            let text = user_input_text(&content);
            if !text.is_empty() {
                messages.push(cx.new(|cx| MessageState::new(Message::User(text), cx)));
            }
        }
        ThreadItem::AgentMessage {
            id, text, phase, ..
        } => {
            messages.push(cx.new(|cx| {
                MessageState::new(
                    Message::Assistant {
                        id,
                        body: text,
                        state: StreamState::Complete,
                        phase: assistant_phase(phase.as_ref()),
                        tools: Vec::new(),
                    },
                    cx,
                )
            }));
        }
        ThreadItem::CommandExecution {
            id,
            command,
            cwd,
            status,
            ..
        } => {
            push_tool_to_messages(
                messages,
                ToolCall::Command {
                    id,
                    command,
                    cwd: cwd.display().to_string(),
                    status: tool_status_from_command(&status),
                },
                cx,
            );
        }
        ThreadItem::FileChange {
            id,
            changes,
            status,
        } => {
            push_tool_to_messages(
                messages,
                ToolCall::FileChange {
                    id,
                    changes: summarize_file_changes(changes),
                    status: tool_status_from_patch(&status),
                },
                cx,
            );
        }
        ThreadItem::McpToolCall {
            id,
            server,
            tool,
            status,
            ..
        } => {
            push_tool_to_messages(
                messages,
                ToolCall::Mcp {
                    id,
                    server,
                    tool,
                    status: tool_status_from_mcp(&status),
                },
                cx,
            );
        }
        ThreadItem::DynamicToolCall {
            id,
            namespace,
            tool,
            status,
            ..
        } => {
            push_tool_to_messages(
                messages,
                ToolCall::Dynamic {
                    id,
                    namespace,
                    tool,
                    status: tool_status_from_dynamic(&status),
                },
                cx,
            );
        }
        _ => {}
    }
}

pub(super) fn push_tool_to_messages(
    messages: &mut Vec<Entity<MessageState>>,
    tool: ToolCall,
    cx: &mut Context<CodexGui>,
) {
    for message in messages.iter().rev() {
        let is_assistant = matches!(&message.read(cx).message, Message::Assistant { .. });
        if is_assistant {
            message.update(cx, |message, cx| {
                if let Message::Assistant { tools, .. } = &mut message.message {
                    tools.push(tool);
                    cx.notify();
                }
            });
            return;
        }
    }

    messages.push(cx.new(|cx| {
        MessageState::new(
            Message::Assistant {
                id: format!("tool-group-{}", tool.id()),
                body: String::new(),
                state: StreamState::Complete,
                phase: AssistantPhase::Commentary,
                tools: vec![tool],
            },
            cx,
        )
    }));
}

pub(super) fn assistant_phase(phase: Option<&MessagePhase>) -> AssistantPhase {
    match phase {
        Some(MessagePhase::Commentary) => AssistantPhase::Commentary,
        Some(MessagePhase::FinalAnswer) | None => AssistantPhase::FinalAnswer,
    }
}

pub(super) fn upsert_chat(
    project: &mut ProjectState,
    chat: Entity<ChatState>,
    chat_id: &str,
    cx: &mut Context<ProjectState>,
) {
    for index in 0..project.chats.len() {
        let is_match = project.chats[index].read(cx).id == chat_id;
        if is_match {
            project.chats[index] = chat;
            return;
        }
    }
    project.chats.insert(0, chat);
}

pub(super) fn thread_status_label(status: &ThreadStatus) -> &'static str {
    match status {
        ThreadStatus::NotLoaded => "not loaded",
        ThreadStatus::Idle => "idle",
        ThreadStatus::SystemError => "system error",
        ThreadStatus::Active { .. } => "active",
    }
}

pub(super) fn tool_status_from_command(status: &CommandExecutionStatus) -> ToolStatus {
    match status {
        CommandExecutionStatus::InProgress => ToolStatus::Running,
        CommandExecutionStatus::Completed
        | CommandExecutionStatus::Failed
        | CommandExecutionStatus::Declined => ToolStatus::Done,
    }
}

pub(super) fn tool_status_from_mcp(status: &McpToolCallStatus) -> ToolStatus {
    match status {
        McpToolCallStatus::InProgress => ToolStatus::Running,
        McpToolCallStatus::Completed | McpToolCallStatus::Failed => ToolStatus::Done,
    }
}

pub(super) fn tool_status_from_dynamic(status: &DynamicToolCallStatus) -> ToolStatus {
    match status {
        DynamicToolCallStatus::InProgress => ToolStatus::Running,
        DynamicToolCallStatus::Completed | DynamicToolCallStatus::Failed => ToolStatus::Done,
    }
}

pub(super) fn tool_status_from_patch(status: &PatchApplyStatus) -> ToolStatus {
    match status {
        PatchApplyStatus::InProgress => ToolStatus::Running,
        PatchApplyStatus::Completed | PatchApplyStatus::Failed | PatchApplyStatus::Declined => {
            ToolStatus::Done
        }
    }
}

pub(super) fn summarize_file_changes(changes: Vec<FileUpdateChange>) -> Vec<FileChangeSummary> {
    changes
        .into_iter()
        .map(|change| {
            let stats = diff_stats(&change.diff);
            FileChangeSummary {
                path: change.path,
                kind: file_change_kind(change.kind),
                additions: stats.additions,
                deletions: stats.deletions,
            }
        })
        .collect()
}

pub(super) fn file_change_kind(kind: PatchChangeKind) -> FileChangeKind {
    match kind {
        PatchChangeKind::Add => FileChangeKind::Add,
        PatchChangeKind::Delete => FileChangeKind::Delete,
        PatchChangeKind::Update { move_path } => FileChangeKind::Update {
            move_path: move_path.map(|path| path.display().to_string()),
        },
    }
}

pub(super) struct DiffStats {
    pub(super) additions: usize,
    pub(super) deletions: usize,
}

pub(super) fn diff_stats(diff: &str) -> DiffStats {
    let mut additions = 0;
    let mut deletions = 0;
    for line in diff.lines() {
        if line.starts_with("+++") || line.starts_with("---") {
            continue;
        }
        if line.starts_with('+') {
            additions += 1;
        } else if line.starts_with('-') {
            deletions += 1;
        }
    }
    DiffStats {
        additions,
        deletions,
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
