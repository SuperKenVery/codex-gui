use crate::models::{Chat, Message, StreamState, ToolCall, ToolStatus};
use crate::workspace::workspace_path;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::Duration,
};

pub enum BridgeCommand {
    StartThread { cwd: String },
    SendTurn { thread_id: String, text: String },
    ForkThread { thread_id: String },
}

pub enum BridgeEvent {
    Status(String),
    ThreadsLoaded(Vec<Chat>),
    ThreadStarted(Chat),
    ThreadForked(Chat),
    TurnStarted {
        thread_id: String,
    },
    UserMessage {
        thread_id: String,
        text: String,
    },
    AgentMessageStarted {
        thread_id: String,
        item_id: String,
        text: String,
    },
    AgentMessageDelta {
        thread_id: String,
        item_id: String,
        delta: String,
    },
    ToolStarted {
        thread_id: String,
        tool: ToolCall,
    },
    ToolOutputDelta {
        thread_id: String,
        item_id: String,
        delta: String,
    },
    ItemCompleted {
        thread_id: String,
        item_id: String,
    },
    Error(String),
}

pub fn start_app_server_bridge() -> (Sender<BridgeCommand>, Receiver<BridgeEvent>) {
    let (command_tx, command_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::channel();

    thread::spawn(move || run_app_server_bridge(command_rx, event_tx));

    (command_tx, event_rx)
}

fn run_app_server_bridge(command_rx: Receiver<BridgeCommand>, event_tx: Sender<BridgeEvent>) {
    let mut child = match Command::new("codex")
        .args(["app-server", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => {
            let _ = event_tx.send(BridgeEvent::Error(format!(
                "Failed to start codex app-server: {err}"
            )));
            return;
        }
    };

    let Some(mut stdin) = child.stdin.take() else {
        let _ = event_tx.send(BridgeEvent::Error(
            "codex app-server stdin unavailable".into(),
        ));
        return;
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = event_tx.send(BridgeEvent::Error(
            "codex app-server stdout unavailable".into(),
        ));
        return;
    };

    if let Some(stderr) = child.stderr.take() {
        let event_tx = event_tx.clone();
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if !line.trim().is_empty() {
                    let _ = event_tx.send(BridgeEvent::Status(line));
                }
            }
        });
    }

    let mut requests = PendingRequests::default();
    let mut next_id = 1_i64;
    let mut send_request = |method: &str, params: Value, requests: &mut PendingRequests| {
        let id = next_id;
        next_id += 1;
        requests.insert(id, method.to_string());
        let request = json!({ "id": id, "method": method, "params": params });
        writeln!(stdin, "{request}")?;
        stdin.flush()
    };

    if let Err(err) = send_request(
        "initialize",
        json!({
            "clientInfo": { "name": "codex-gui", "version": env!("CARGO_PKG_VERSION") },
            "capabilities": null
        }),
        &mut requests,
    ) {
        let _ = event_tx.send(BridgeEvent::Error(format!(
            "Failed to initialize codex app-server: {err}"
        )));
        return;
    }

    if let Err(err) = send_request(
        "thread/list",
        json!({
            "limit": 30,
            "cwd": workspace_path(),
            "archived": false,
            "useStateDbOnly": false
        }),
        &mut requests,
    ) {
        let _ = event_tx.send(BridgeEvent::Error(format!(
            "Failed to list codex threads: {err}"
        )));
    }

    let (line_tx, line_rx) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = line_tx.send(line);
        }
    });

    loop {
        while let Ok(command) = command_rx.try_recv() {
            let result = match command {
                BridgeCommand::StartThread { cwd } => send_request(
                    "thread/start",
                    json!({
                        "cwd": cwd,
                        "approvalPolicy": "on-request",
                        "sandbox": "workspace-write",
                        "threadSource": "user"
                    }),
                    &mut requests,
                ),
                BridgeCommand::SendTurn { thread_id, text } => send_request(
                    "turn/start",
                    json!({
                        "threadId": thread_id,
                        "input": [{ "type": "text", "text": text, "text_elements": [] }]
                    }),
                    &mut requests,
                ),
                BridgeCommand::ForkThread { thread_id } => send_request(
                    "thread/fork",
                    json!({ "threadId": thread_id }),
                    &mut requests,
                ),
            };
            if let Err(err) = result {
                let _ = event_tx.send(BridgeEvent::Error(format!(
                    "Failed to send codex app-server request: {err}"
                )));
            }
        }

        match line_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(line) => handle_server_line(&line, &event_tx, &mut requests),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = event_tx.send(BridgeEvent::Error(
                    "codex app-server stdout stream closed".into(),
                ));
                break;
            }
        }
    }

    let _ = child.kill();
}

#[derive(Default)]
struct PendingRequests(HashMap<i64, String>);

impl PendingRequests {
    fn insert(&mut self, id: i64, method: String) {
        self.0.insert(id, method);
    }

    fn remove(&mut self, id: i64) -> Option<String> {
        self.0.remove(&id)
    }
}

fn handle_server_line(line: &str, event_tx: &Sender<BridgeEvent>, requests: &mut PendingRequests) {
    let parsed: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(err) => {
            let _ = event_tx.send(BridgeEvent::Error(format!(
                "Invalid app-server JSON: {err}: {line}"
            )));
            return;
        }
    };

    if let Some(error) = parsed.get("error") {
        let _ = event_tx.send(BridgeEvent::Error(format!("app-server error: {error}")));
        return;
    }

    if let Some(id) = parsed.get("id").and_then(Value::as_i64) {
        let method = requests.remove(id).unwrap_or_default();
        let result = parsed.get("result").cloned().unwrap_or(Value::Null);
        handle_response(&method, result, event_tx);
        return;
    }

    if let Some(method) = parsed.get("method").and_then(Value::as_str) {
        let params = parsed.get("params").cloned().unwrap_or(Value::Null);
        handle_notification(method, params, event_tx);
    }
}

fn handle_response(method: &str, result: Value, event_tx: &Sender<BridgeEvent>) {
    match method {
        "initialize" => {
            let user_agent = result
                .get("userAgent")
                .and_then(Value::as_str)
                .unwrap_or("Codex app-server");
            let _ = event_tx.send(BridgeEvent::Status(format!("connected: {user_agent}")));
        }
        "thread/list" => {
            let chats = result
                .get("data")
                .and_then(Value::as_array)
                .map(|threads| threads.iter().map(chat_from_thread).collect())
                .unwrap_or_default();
            let _ = event_tx.send(BridgeEvent::ThreadsLoaded(chats));
        }
        "thread/start" => {
            if let Some(thread) = result.get("thread") {
                let _ = event_tx.send(BridgeEvent::ThreadStarted(chat_from_thread(thread)));
            }
        }
        "thread/fork" => {
            if let Some(thread) = result.get("thread") {
                let _ = event_tx.send(BridgeEvent::ThreadForked(chat_from_thread(thread)));
            }
        }
        "turn/start" => {
            if let Some(turn) = result.get("turn") {
                if let Some(thread_id) = turn.get("threadId").and_then(Value::as_str) {
                    let _ = event_tx.send(BridgeEvent::TurnStarted {
                        thread_id: thread_id.into(),
                    });
                }
            }
        }
        _ => {}
    }
}

fn handle_notification(method: &str, params: Value, event_tx: &Sender<BridgeEvent>) {
    match method {
        "thread/started" => {
            if let Some(thread) = params.get("thread") {
                let _ = event_tx.send(BridgeEvent::ThreadStarted(chat_from_thread(thread)));
            }
        }
        "turn/started" => {
            if let Some(thread_id) = params.get("threadId").and_then(Value::as_str) {
                let _ = event_tx.send(BridgeEvent::TurnStarted {
                    thread_id: thread_id.into(),
                });
            }
        }
        "item/started" => {
            if let (Some(thread_id), Some(item)) = (
                params.get("threadId").and_then(Value::as_str),
                params.get("item"),
            ) {
                emit_item_started(thread_id, item, event_tx);
            }
        }
        "item/agentMessage/delta" => {
            if let (Some(thread_id), Some(item_id), Some(delta)) = (
                params.get("threadId").and_then(Value::as_str),
                params.get("itemId").and_then(Value::as_str),
                params.get("delta").and_then(Value::as_str),
            ) {
                let _ = event_tx.send(BridgeEvent::AgentMessageDelta {
                    thread_id: thread_id.into(),
                    item_id: item_id.into(),
                    delta: delta.into(),
                });
            }
        }
        "item/commandExecution/outputDelta" | "command/exec/outputDelta" => {
            if let (Some(thread_id), Some(item_id), Some(delta)) = (
                params.get("threadId").and_then(Value::as_str),
                params.get("itemId").and_then(Value::as_str),
                params.get("delta").and_then(Value::as_str),
            ) {
                let _ = event_tx.send(BridgeEvent::ToolOutputDelta {
                    thread_id: thread_id.into(),
                    item_id: item_id.into(),
                    delta: delta.into(),
                });
            }
        }
        "item/completed" => {
            if let (Some(thread_id), Some(item)) = (
                params.get("threadId").and_then(Value::as_str),
                params.get("item"),
            ) {
                emit_item_completed(thread_id, item, event_tx);
            }
        }
        "thread/status/changed" => {
            if let Some(status) = params.get("status") {
                let _ = event_tx.send(BridgeEvent::Status(format!(
                    "thread {}",
                    status_label(status)
                )));
            }
        }
        "turn/completed" => {
            let _ = event_tx.send(BridgeEvent::Status("turn complete".into()));
        }
        "error" => {
            let message = params
                .get("message")
                .and_then(Value::as_str)
                .map(String::from)
                .unwrap_or_else(|| format!("{params}"));
            let _ = event_tx.send(BridgeEvent::Error(message));
        }
        _ => {}
    }
}

fn emit_item_started(thread_id: &str, item: &Value, event_tx: &Sender<BridgeEvent>) {
    let Some(item_type) = item.get("type").and_then(Value::as_str) else {
        return;
    };
    let item_id = item
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("unknown-item")
        .to_string();

    match item_type {
        "userMessage" => {
            let text = user_input_text(item.get("content"));
            let _ = event_tx.send(BridgeEvent::UserMessage {
                thread_id: thread_id.into(),
                text,
            });
        }
        "agentMessage" => {
            let text = item
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let _ = event_tx.send(BridgeEvent::AgentMessageStarted {
                thread_id: thread_id.into(),
                item_id,
                text,
            });
        }
        "commandExecution" => {
            let command = item
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or("command")
                .to_string();
            let cwd = item
                .get("cwd")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let _ = event_tx.send(BridgeEvent::ToolStarted {
                thread_id: thread_id.into(),
                tool: ToolCall {
                    id: item_id,
                    name: command,
                    status: ToolStatus::Running,
                    detail: cwd,
                },
            });
        }
        "mcpToolCall" | "dynamicToolCall" => {
            let name = item
                .get("tool")
                .and_then(Value::as_str)
                .unwrap_or(item_type)
                .to_string();
            let _ = event_tx.send(BridgeEvent::ToolStarted {
                thread_id: thread_id.into(),
                tool: ToolCall {
                    id: item_id,
                    name,
                    status: ToolStatus::Running,
                    detail: "tool call started".into(),
                },
            });
        }
        _ => {}
    }
}

fn emit_item_completed(thread_id: &str, item: &Value, event_tx: &Sender<BridgeEvent>) {
    if let Some(item_id) = item.get("id").and_then(Value::as_str) {
        let _ = event_tx.send(BridgeEvent::ItemCompleted {
            thread_id: thread_id.into(),
            item_id: item_id.into(),
        });
    }
}

fn chat_from_thread(thread: &Value) -> Chat {
    let id = thread
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("unknown-thread")
        .to_string();
    let title = thread
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .or_else(|| thread.get("preview").and_then(Value::as_str))
        .filter(|preview| !preview.is_empty())
        .unwrap_or("Untitled Codex thread");
    let cwd = thread
        .get("cwd")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let subtitle = format!("{} - {}", status_label(&thread["status"]), cwd);
    let mut chat = Chat {
        id,
        title: title.into(),
        subtitle: subtitle.into(),
        messages: Vec::new(),
    };

    if let Some(turns) = thread.get("turns").and_then(Value::as_array) {
        for turn in turns {
            if let Some(items) = turn.get("items").and_then(Value::as_array) {
                for item in items {
                    append_thread_item(&mut chat, item);
                }
            }
        }
    }

    chat
}

fn status_label(status: &Value) -> String {
    status
        .as_str()
        .or_else(|| status.get("type").and_then(Value::as_str))
        .unwrap_or("unknown")
        .to_string()
}

fn append_thread_item(chat: &mut Chat, item: &Value) {
    match item.get("type").and_then(Value::as_str) {
        Some("userMessage") => {
            let text = user_input_text(item.get("content"));
            if !text.is_empty() {
                chat.messages.push(Message::User(text));
            }
        }
        Some("agentMessage") => {
            let id = item
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("agent-message")
                .to_string();
            let body = item
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            chat.messages.push(Message::Assistant {
                id,
                body,
                state: StreamState::Complete,
                tools: Vec::new(),
            });
        }
        Some("commandExecution") => {
            let id = item
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("command")
                .to_string();
            let name = item
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or("command")
                .to_string();
            let detail = item
                .get("aggregatedOutput")
                .and_then(Value::as_str)
                .or_else(|| item.get("cwd").and_then(Value::as_str))
                .unwrap_or("")
                .to_string();
            let tool = ToolCall {
                id,
                name,
                status: ToolStatus::Done,
                detail,
            };
            if let Some(Message::Assistant { tools, .. }) = chat
                .messages
                .iter_mut()
                .rev()
                .find(|message| matches!(message, Message::Assistant { .. }))
            {
                tools.push(tool);
            } else {
                chat.messages.push(Message::Assistant {
                    id: format!("tool-group-{}", tool.id),
                    body: "Codex used a tool.".into(),
                    state: StreamState::Complete,
                    tools: vec![tool],
                });
            }
        }
        _ => {}
    }
}

fn user_input_text(content: Option<&Value>) -> String {
    content
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    if item.get("type").and_then(Value::as_str) == Some("text") {
                        item.get("text").and_then(Value::as_str)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

pub fn empty_chat() -> Chat {
    Chat {
        id: "empty".into(),
        title: "No Codex threads".into(),
        subtitle: "Click New to start one in this workspace".into(),
        messages: vec![Message::Commentary(
            "No persisted Codex threads were returned for this workspace.".into(),
        )],
    }
}
