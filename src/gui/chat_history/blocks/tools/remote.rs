use codex_app_server_protocol::ThreadItem;
use gpui::SharedString;
use gpui_component::IconName;

use super::simple::{SimpleTool, ToolStatus, append_progress, format_json};

#[derive(Clone)]
pub(in crate::gui::chat_history) struct McpTool {
    title: SharedString,
    detail: Option<SharedString>,
    status: ToolStatus,
}

impl McpTool {
    pub(super) fn new(
        item: &ThreadItem,
        status: ToolStatus,
        progress: Option<&[SharedString]>,
    ) -> Option<Self> {
        let ThreadItem::McpToolCall {
            server,
            tool,
            arguments,
            result,
            error,
            duration_ms,
            ..
        } = item
        else {
            return None;
        };
        let action = if matches!(status, ToolStatus::Running) {
            "Calling"
        } else {
            "Called"
        };
        let mut details = vec![format_json(arguments)];
        if let Some(result) = result {
            details.push(format!("result:\n{}", format_json(result.as_ref())));
        }
        if let Some(error) = error {
            details.push(format!("error:\n{}", format_json(error)));
        }
        if let Some(duration_ms) = duration_ms {
            details.push(format!("duration: {duration_ms} ms"));
        }
        Some(Self {
            title: format!("{action} {server}.{tool}").into(),
            detail: append_progress(Some(details.join("\n\n")), progress),
            status,
        })
    }
}

impl SimpleTool for McpTool {
    fn icon(&self) -> IconName {
        IconName::Globe
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

#[derive(Clone)]
pub(in crate::gui::chat_history) struct DynamicTool {
    title: SharedString,
    detail: Option<SharedString>,
    status: ToolStatus,
}

impl DynamicTool {
    pub(super) fn new(
        item: &ThreadItem,
        status: ToolStatus,
        progress: Option<&[SharedString]>,
    ) -> Option<Self> {
        let ThreadItem::DynamicToolCall {
            namespace,
            tool,
            arguments,
            content_items,
            duration_ms,
            ..
        } = item
        else {
            return None;
        };
        let name = namespace
            .as_ref()
            .map(|namespace| format!("{namespace}.{tool}"))
            .unwrap_or_else(|| tool.clone());
        let action = if matches!(status, ToolStatus::Running) {
            "Calling"
        } else {
            "Called"
        };
        let mut details = vec![format_json(arguments)];
        if let Some(content_items) = content_items {
            details.push(format!("output:\n{}", format_json(content_items)));
        }
        if let Some(duration_ms) = duration_ms {
            details.push(format!("duration: {duration_ms} ms"));
        }
        Some(Self {
            title: format!("{action} {name}").into(),
            detail: append_progress(Some(details.join("\n\n")), progress),
            status,
        })
    }
}

impl SimpleTool for DynamicTool {
    fn icon(&self) -> IconName {
        IconName::Asterisk
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
