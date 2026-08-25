use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use codex_code_mode_protocol::CodeModeNestedToolCall;
use codex_code_mode_protocol::CodeModeSessionDelegate;
use codex_code_mode_protocol::ToolDefinition;
use serde_json::Value as JsonValue;
use tokio::sync::Notify;

use crate::cell::Cell;

pub(crate) struct RuntimeBridge {
    delegate: Arc<dyn CodeModeSessionDelegate>,
    cell: Arc<Cell>,
    tool_call_id: String,
    enabled_tools: Vec<ToolDefinition>,
    stored_values: Mutex<HashMap<String, JsonValue>>,
    stored_value_writes: Mutex<HashMap<String, JsonValue>>,
    next_tool_call_id: AtomicU64,
    exit_requested: AtomicBool,
    pending_notifications: AtomicUsize,
    notifications_changed: Notify,
}

impl RuntimeBridge {
    pub(crate) fn new(
        delegate: Arc<dyn CodeModeSessionDelegate>,
        cell: Arc<Cell>,
        tool_call_id: String,
        enabled_tools: Vec<ToolDefinition>,
        stored_values: HashMap<String, JsonValue>,
    ) -> Self {
        Self {
            delegate,
            cell,
            tool_call_id,
            enabled_tools,
            stored_values: Mutex::new(stored_values),
            stored_value_writes: Mutex::new(HashMap::new()),
            next_tool_call_id: AtomicU64::new(1),
            exit_requested: AtomicBool::new(false),
            pending_notifications: AtomicUsize::new(0),
            notifications_changed: Notify::new(),
        }
    }

    pub(crate) fn cell(&self) -> &Arc<Cell> {
        &self.cell
    }

    pub(crate) fn enabled_tools(&self) -> &[ToolDefinition] {
        &self.enabled_tools
    }

    pub(crate) fn request_exit(&self) {
        self.exit_requested.store(true, Ordering::Release);
    }

    pub(crate) fn exit_requested(&self) -> bool {
        self.exit_requested.load(Ordering::Acquire)
    }

    pub(crate) async fn invoke_tool(&self, index: i32, input_json: Option<String>) -> String {
        let Some(tool) = usize::try_from(index)
            .ok()
            .and_then(|index| self.enabled_tools.get(index))
            .cloned()
        else {
            return error_envelope("invalid code-mode tool index");
        };
        let input = match input_json {
            Some(json) => match serde_json::from_str(&json) {
                Ok(value) => Some(value),
                Err(error) => return error_envelope(&format!("invalid tool input: {error}")),
            },
            None => None,
        };
        let runtime_tool_call_id = format!(
            "tool-{}",
            self.next_tool_call_id.fetch_add(1, Ordering::Relaxed)
        );
        let result = self
            .delegate
            .invoke_tool(
                CodeModeNestedToolCall {
                    cell_id: self.cell.id.clone(),
                    runtime_tool_call_id,
                    tool_name: tool.tool_name,
                    tool_kind: tool.kind,
                    input,
                },
                self.cell.cancellation.child_token(),
            )
            .await;
        match result {
            Ok(value) => serde_json::json!({ "ok": true, "value": value }).to_string(),
            Err(error) => error_envelope(&error),
        }
    }

    pub(crate) fn notify(self: &Arc<Self>, text: String) {
        self.pending_notifications.fetch_add(1, Ordering::AcqRel);
        let bridge = Arc::clone(self);
        tokio::spawn(async move {
            let result = bridge
                .delegate
                .notify(
                    bridge.tool_call_id.clone(),
                    bridge.cell.id.clone(),
                    text,
                    bridge.cell.cancellation.child_token(),
                )
                .await;
            if let Err(error) = result {
                tracing::warn!(%error, "QuickJS code-mode notification failed");
            }
            bridge.pending_notifications.fetch_sub(1, Ordering::AcqRel);
            bridge.notifications_changed.notify_waiters();
        });
    }

    pub(crate) async fn wait_for_notifications(&self) {
        loop {
            let changed = self.notifications_changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.pending_notifications.load(Ordering::Acquire) == 0 {
                return;
            }
            changed.await;
        }
    }

    pub(crate) fn store(&self, key: String, json: String) -> Result<(), String> {
        let value: JsonValue = serde_json::from_str(&json)
            .map_err(|error| format!("failed to serialize stored value: {error}"))?;
        self.stored_values
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key.clone(), value.clone());
        self.stored_value_writes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key, value);
        Ok(())
    }

    pub(crate) fn load(&self, key: &str) -> Option<String> {
        self.stored_values
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(key)
            .and_then(|value| serde_json::to_string(value).ok())
    }

    pub(crate) fn stored_value_writes(&self) -> HashMap<String, JsonValue> {
        self.stored_value_writes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

fn error_envelope(error: &str) -> String {
    serde_json::json!({ "ok": false, "error": error }).to_string()
}
