use codex_app_server_protocol::ThreadItem;
use gpui::SharedString;
use gpui_component::IconName;

use super::simple::{SimpleTool, ToolStatus};

#[derive(Clone)]
pub(in crate::gui::chat_history) struct WebSearchTool {
    query: SharedString,
    status: ToolStatus,
}

impl WebSearchTool {
    pub(super) fn new(item: &ThreadItem, status: ToolStatus) -> Option<Self> {
        let ThreadItem::WebSearch(item) = item else {
            return None;
        };
        Some(Self {
            query: item.query.clone().into(),
            status,
        })
    }
}

impl SimpleTool for WebSearchTool {
    fn icon(&self) -> IconName {
        IconName::Search
    }

    fn title(&self) -> SharedString {
        self.query.clone()
    }

    fn detail(&self) -> Option<SharedString> {
        None
    }

    fn status(&self) -> ToolStatus {
        self.status
    }
}
