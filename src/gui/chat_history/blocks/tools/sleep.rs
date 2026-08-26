use codex_app_server_protocol::ThreadItem;
use gpui::SharedString;
use gpui_component::IconName;

use super::simple::{SimpleTool, ToolStatus};

#[derive(Clone)]
pub(in crate::gui::chat_history) struct SleepTool {
    duration_ms: u64,
    status: ToolStatus,
}

impl SleepTool {
    pub(super) fn new(item: &ThreadItem, status: ToolStatus) -> Option<Self> {
        let ThreadItem::Sleep(item) = item else {
            return None;
        };
        Some(Self {
            duration_ms: item.duration_ms,
            status,
        })
    }
}

impl SimpleTool for SleepTool {
    fn icon(&self) -> IconName {
        IconName::Pause
    }

    fn title(&self) -> SharedString {
        let action = if matches!(self.status, ToolStatus::Running) {
            "Waiting"
        } else {
            "Waited"
        };
        format!("{action} {}", format_duration_ms(self.duration_ms)).into()
    }

    fn detail(&self) -> Option<SharedString> {
        None
    }

    fn status(&self) -> ToolStatus {
        self.status
    }
}

fn format_duration_ms(duration_ms: u64) -> String {
    if duration_ms < 1_000 {
        format!("{duration_ms} ms")
    } else if duration_ms % 1_000 == 0 {
        format!("{} s", duration_ms / 1_000)
    } else {
        format!("{:.1} s", duration_ms as f64 / 1_000.)
    }
}
