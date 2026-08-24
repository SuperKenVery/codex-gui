use std::{collections::HashSet, time::Duration};

use codex_app_server_protocol::{ThreadItem, Turn, TurnStatus, UserInput};
use codex_protocol::models::MessagePhase;

use crate::gui::{ChatState, PendingUserMessageDelivery};

use super::{
    blocks::{HistoryBlock, UserMessageDelivery, is_tool_item, tool_call_views, tools_done},
    transcript::TranscriptSnapshot,
};

pub(super) fn build_transcript(
    chat: &ChatState,
    expanded_turns: &HashSet<String>,
    expanded_tool_groups: &HashSet<String>,
) -> TranscriptSnapshot {
    let mut transcript = TranscriptSnapshot::new();

    if let Some(thread) = &chat.thread {
        let mut previous_turn_id = None;
        for turn in &thread.turns {
            append_turn(
                &mut transcript,
                chat,
                turn,
                previous_turn_id.as_deref(),
                expanded_turns,
                expanded_tool_groups,
            );
            previous_turn_id = Some(turn.id.as_str());
        }
    }

    if let Some(message) = chat.pending_user_message() {
        let body = user_input_text(&message.content);
        if !body.is_empty() {
            let delivery = match &message.delivery {
                PendingUserMessageDelivery::Sending => UserMessageDelivery::Sending,
                PendingUserMessageDelivery::Failed(_) => UserMessageDelivery::Failed,
            };
            transcript.push_block(HistoryBlock::User {
                key: message.client_id.clone(),
                turn_id: None,
                previous_turn_id: None,
                body: body.into(),
                delivery,
            });
        }
    }

    for notice in &chat.notices {
        transcript.push_block(HistoryBlock::Notice {
            key: notice.id.clone(),
            body: notice.body.clone(),
        });
    }

    transcript
}

fn append_turn(
    transcript: &mut TranscriptSnapshot,
    chat: &ChatState,
    turn: &Turn,
    previous_turn_id: Option<&str>,
    expanded_turns: &HashSet<String>,
    expanded_tool_groups: &HashSet<String>,
) {
    let Some(fold) = completed_turn_fold(turn) else {
        append_items(
            transcript,
            chat,
            &turn.id,
            previous_turn_id,
            &turn.items,
            expanded_tool_groups,
        );
        return;
    };

    append_items(
        transcript,
        chat,
        &turn.id,
        previous_turn_id,
        &turn.items[..=fold.user_index],
        expanded_tool_groups,
    );

    let expanded = expanded_turns.contains(&turn.id);
    transcript.push_block(HistoryBlock::WorkedSummary {
        turn_id: turn.id.clone(),
        duration: turn_duration(turn),
        expanded,
    });

    if expanded {
        append_items(
            transcript,
            chat,
            &turn.id,
            previous_turn_id,
            &turn.items[fold.user_index + 1..],
            expanded_tool_groups,
        );
    } else if let Some(final_answer) = turn.items.get(fold.final_index) {
        append_agent(
            transcript,
            chat,
            final_answer,
            &[],
            false,
            expanded_tool_groups,
        );
    }
}

fn append_items(
    transcript: &mut TranscriptSnapshot,
    chat: &ChatState,
    turn_id: &str,
    previous_turn_id: Option<&str>,
    items: &[ThreadItem],
    expanded_tool_groups: &HashSet<String>,
) {
    let mut index = 0;
    while index < items.len() {
        match &items[index] {
            ThreadItem::UserMessage {
                id,
                client_id,
                content,
            } => {
                let body = user_input_text(content);
                if !body.is_empty() {
                    transcript.push_block(HistoryBlock::User {
                        key: client_id.clone().unwrap_or_else(|| id.clone()),
                        turn_id: Some(turn_id.to_string()),
                        previous_turn_id: previous_turn_id.map(str::to_string),
                        body: body.into(),
                        delivery: UserMessageDelivery::Sent,
                    });
                }
                index += 1;
            }
            ThreadItem::AgentMessage { .. } => {
                let tools_end = consecutive_tools_end(items, index + 1);
                append_agent(
                    transcript,
                    chat,
                    &items[index],
                    &items[index + 1..tools_end],
                    tools_end == items.len(),
                    expanded_tool_groups,
                );
                index = tools_end;
            }
            item if is_tool_item(item) => {
                let tools_end = consecutive_tools_end(items, index);
                append_tool_group(
                    transcript,
                    item.id(),
                    &items[index..tools_end],
                    tools_end == items.len()
                        && !tools_done(&items[index..tools_end].iter().collect::<Vec<_>>()),
                    expanded_tool_groups,
                );
                index = tools_end;
            }
            _ => index += 1,
        }
    }
}

fn append_agent(
    transcript: &mut TranscriptSnapshot,
    chat: &ChatState,
    item: &ThreadItem,
    tools: &[ThreadItem],
    tool_group_at_tail: bool,
    expanded_tool_groups: &HashSet<String>,
) {
    let ThreadItem::AgentMessage {
        id, text, phase, ..
    } = item
    else {
        return;
    };

    let label = match phase.as_ref() {
        Some(MessagePhase::Commentary) => "",
        _ if chat.item_is_streaming(id) => "Codex is working",
        _ => "Codex",
    };
    if !label.is_empty() {
        transcript.push_block(HistoryBlock::AssistantHeader {
            key: id.clone(),
            label,
        });
    }
    transcript.push_markdown(text);

    if !tools.is_empty() {
        append_tool_group(
            transcript,
            id,
            tools,
            tool_group_at_tail && !tools_done(&tools.iter().collect::<Vec<_>>()),
            expanded_tool_groups,
        );
    }
}

fn append_tool_group(
    transcript: &mut TranscriptSnapshot,
    key: &str,
    tools: &[ThreadItem],
    active_tail: bool,
    expanded_tool_groups: &HashSet<String>,
) {
    let tool_refs = tools.iter().collect::<Vec<_>>();
    transcript.push_block(HistoryBlock::ToolGroup {
        key: key.to_string(),
        tools: tool_call_views(&tool_refs),
        expanded: expanded_tool_groups.contains(key),
        collapsible: true,
        active_tail,
    });
}

fn consecutive_tools_end(items: &[ThreadItem], start: usize) -> usize {
    let mut end = start;
    while end < items.len() && is_tool_item(&items[end]) {
        end += 1;
    }
    end
}

struct TurnFold {
    user_index: usize,
    final_index: usize,
}

fn completed_turn_fold(turn: &Turn) -> Option<TurnFold> {
    if !matches!(turn.status, TurnStatus::Completed) {
        return None;
    }

    let user_indices = turn
        .items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| matches!(item, ThreadItem::UserMessage { .. }).then_some(index))
        .collect::<Vec<_>>();
    let [user_index] = user_indices.as_slice() else {
        return None;
    };

    let final_index = turn
        .items
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, item)| {
            let ThreadItem::AgentMessage { text, phase, .. } = item else {
                return None;
            };
            (!text.trim().is_empty() && is_final_answer(phase.as_ref())).then_some(index)
        })?;
    if final_index <= *user_index {
        return None;
    }

    let has_progress = turn.items[*user_index + 1..]
        .iter()
        .enumerate()
        .any(|(offset, item)| {
            let index = *user_index + 1 + offset;
            index != final_index
                && (is_tool_item(item)
                    || matches!(item, ThreadItem::AgentMessage { text, .. } if !text.trim().is_empty()))
        });
    if !has_progress {
        return None;
    }

    let tools = turn
        .items
        .iter()
        .filter(|item| is_tool_item(item))
        .collect::<Vec<_>>();
    if !tools.is_empty() && !tools_done(&tools) {
        return None;
    }

    Some(TurnFold {
        user_index: *user_index,
        final_index,
    })
}

fn turn_duration(turn: &Turn) -> Duration {
    if let Some(duration_ms) = turn
        .duration_ms
        .and_then(|duration| u64::try_from(duration).ok())
    {
        return Duration::from_millis(duration_ms);
    }

    let seconds = turn
        .started_at
        .zip(turn.completed_at)
        .map(|(started, completed)| completed.saturating_sub(started))
        .and_then(|duration| u64::try_from(duration).ok())
        .unwrap_or_default();
    Duration::from_secs(seconds)
}

fn is_final_answer(phase: Option<&MessagePhase>) -> bool {
    matches!(phase, Some(MessagePhase::FinalAnswer) | None)
}

fn user_input_text(content: &[UserInput]) -> String {
    content
        .iter()
        .filter_map(|input| match input {
            UserInput::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}
