use codex_app_server_protocol::{CollabAgentTool, ThreadItem};
use gpui::SharedString;
use gpui_component::IconName;

use super::simple::{SimpleTool, ToolStatus, append_progress};

#[derive(Clone)]
pub(in crate::gui::chat_history) struct CollaborationTool {
    title: SharedString,
    detail: Option<SharedString>,
    status: ToolStatus,
}

impl CollaborationTool {
    pub(super) fn new(
        item: &ThreadItem,
        status: ToolStatus,
        progress: Option<&[SharedString]>,
    ) -> Option<Self> {
        let ThreadItem::CollabAgentToolCall {
            tool,
            receiver_thread_ids,
            prompt,
            model,
            reasoning_effort,
            ..
        } = item
        else {
            return None;
        };
        let mut details = Vec::new();
        if !receiver_thread_ids.is_empty() {
            details.push(format!("agents: {}", receiver_thread_ids.join(", ")));
        }
        if let Some(model) = model {
            details.push(format!("model: {model}"));
        }
        if let Some(reasoning_effort) = reasoning_effort {
            details.push(format!("reasoning effort: {reasoning_effort:?}"));
        }
        if let Some(prompt) = prompt {
            details.push(prompt.clone());
        }
        Some(Self {
            title: collaboration_title(tool, status).into(),
            detail: append_progress((!details.is_empty()).then(|| details.join("\n")), progress),
            status,
        })
    }
}

impl SimpleTool for CollaborationTool {
    fn icon(&self) -> IconName {
        IconName::Bot
    }

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn detail(&self) -> Option<SharedString> {
        self.detail.clone()
    }

    fn status(&self) -> ToolStatus {
        self.status
    }
}

fn collaboration_title(tool: &CollabAgentTool, status: ToolStatus) -> &'static str {
    match status {
        ToolStatus::Running => match tool {
            CollabAgentTool::SpawnAgent => "Spawning agent",
            CollabAgentTool::SendInput => "Sending input to agent",
            CollabAgentTool::ResumeAgent => "Resuming agent",
            CollabAgentTool::Wait => "Waiting for agents",
            CollabAgentTool::CloseAgent => "Closing agent",
        },
        ToolStatus::Succeeded => match tool {
            CollabAgentTool::SpawnAgent => "Spawned agent",
            CollabAgentTool::SendInput => "Sent input to agent",
            CollabAgentTool::ResumeAgent => "Resumed agent",
            CollabAgentTool::Wait => "Waited for agents",
            CollabAgentTool::CloseAgent => "Closed agent",
        },
        ToolStatus::Failed => match tool {
            CollabAgentTool::SpawnAgent => "Failed to spawn agent",
            CollabAgentTool::SendInput => "Failed to send input to agent",
            CollabAgentTool::ResumeAgent => "Failed to resume agent",
            CollabAgentTool::Wait => "Failed while waiting for agents",
            CollabAgentTool::CloseAgent => "Failed to close agent",
        },
    }
}
