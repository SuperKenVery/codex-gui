use codex_app_server_protocol::ThreadItem;
use gpui::{App, IntoElement, RenderOnce, SharedString, Window};
use gpui_component::IconName;

use super::simple::{ToolFrame, ToolStatus, append_progress};

#[derive(Clone, IntoElement)]
pub(in crate::gui::chat_history) struct CommandTool {
    title: SharedString,
    detail: Option<SharedString>,
    status: ToolStatus,
}

impl CommandTool {
    pub(super) fn new(
        item: &ThreadItem,
        status: ToolStatus,
        progress: Option<&[SharedString]>,
    ) -> Option<Self> {
        let ThreadItem::CommandExecution {
            command,
            cwd,
            aggregated_output,
            exit_code,
            duration_ms,
            ..
        } = item
        else {
            return None;
        };
        let action = if matches!(status, ToolStatus::Running) {
            "Running"
        } else {
            "Ran"
        };
        let mut details = vec![format!("cwd: {cwd}")];
        if let Some(output) = aggregated_output {
            details.push(output.clone());
        }
        if let Some(exit_code) = exit_code {
            details.push(format!("exit code: {exit_code}"));
        }
        if let Some(duration_ms) = duration_ms {
            details.push(format!("duration: {duration_ms} ms"));
        }
        Some(Self {
            title: format!("{action} {}", single_line(command)).into(),
            detail: append_progress(Some(details.join("\n")), progress),
            status,
        })
    }

    pub(super) fn status(&self) -> ToolStatus {
        self.status
    }
}

impl RenderOnce for CommandTool {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        ToolFrame::new(
            IconName::SquareTerminal,
            self.title,
            self.detail,
            self.status,
        )
    }
}

fn single_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
