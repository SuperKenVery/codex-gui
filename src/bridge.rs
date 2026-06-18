use crate::workspace::workspace_path;
use codex_app_server_protocol::{
    AskForApproval, ClientInfo, InitializeParams, InitializeResponse, JSONRPCError, JSONRPCMessage,
    JSONRPCNotification, JSONRPCRequest, JSONRPCResponse, RequestId, SandboxMode,
    ServerNotification, Thread, ThreadForkParams, ThreadForkResponse, ThreadItem,
    ThreadListCwdFilter, ThreadListParams, ThreadListResponse, ThreadResumeParams,
    ThreadResumeResponse, ThreadSource, ThreadStartParams, ThreadStartResponse, ThreadStatus,
    TurnStartParams, TurnStartResponse, UserInput,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
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
    ResumeThread { thread_id: String },
    SendTurn { thread_id: String, text: String },
    ForkThread { thread_id: String },
}

pub enum BridgeEvent {
    Status(String),
    ThreadsLoaded(Vec<Thread>),
    ThreadStarted(Thread),
    ThreadResumed(Thread),
    ThreadForked(Thread),
    TurnStarted {
        thread_id: String,
    },
    ItemStarted {
        thread_id: String,
        item: ThreadItem,
    },
    AgentMessageDelta {
        thread_id: String,
        item_id: String,
        delta: String,
    },
    ToolOutputDelta {
        thread_id: String,
        item_id: String,
        delta: String,
    },
    ItemCompleted {
        thread_id: String,
        item: ThreadItem,
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

    if let Err(err) = send_request(
        "initialize",
        InitializeParams {
            client_info: ClientInfo {
                name: "codex-gui".into(),
                title: Some("codex-gui".into()),
                version: env!("CARGO_PKG_VERSION").into(),
            },
            capabilities: None,
        },
        &mut requests,
        &mut next_id,
        &mut stdin,
    ) {
        let _ = event_tx.send(BridgeEvent::Error(format!(
            "Failed to initialize codex app-server: {err}"
        )));
        return;
    }

    if let Err(err) = send_request(
        "thread/list",
        ThreadListParams {
            cursor: None,
            limit: Some(30),
            sort_key: None,
            sort_direction: None,
            model_providers: None,
            source_kinds: None,
            archived: Some(false),
            cwd: Some(ThreadListCwdFilter::One(workspace_path())),
            use_state_db_only: false,
            search_term: None,
        },
        &mut requests,
        &mut next_id,
        &mut stdin,
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
                    ThreadStartParams {
                        cwd: Some(cwd),
                        approval_policy: Some(AskForApproval::OnRequest),
                        sandbox: Some(SandboxMode::WorkspaceWrite),
                        thread_source: Some(ThreadSource::User),
                        ..Default::default()
                    },
                    &mut requests,
                    &mut next_id,
                    &mut stdin,
                ),
                BridgeCommand::ResumeThread { thread_id } => send_request(
                    "thread/resume",
                    ThreadResumeParams {
                        thread_id,
                        ..Default::default()
                    },
                    &mut requests,
                    &mut next_id,
                    &mut stdin,
                ),
                BridgeCommand::SendTurn { thread_id, text } => send_request(
                    "turn/start",
                    TurnStartParams {
                        thread_id,
                        client_user_message_id: None,
                        input: vec![UserInput::Text {
                            text,
                            text_elements: Vec::new(),
                        }],
                        responsesapi_client_metadata: None,
                        additional_context: None,
                        environments: None,
                        cwd: None,
                        runtime_workspace_roots: None,
                        approval_policy: None,
                        approvals_reviewer: None,
                        sandbox_policy: None,
                        permissions: None,
                        model: None,
                        service_tier: None,
                        effort: None,
                        summary: None,
                        personality: None,
                        output_schema: None,
                        collaboration_mode: None,
                    },
                    &mut requests,
                    &mut next_id,
                    &mut stdin,
                ),
                BridgeCommand::ForkThread { thread_id } => send_request(
                    "thread/fork",
                    ThreadForkParams {
                        thread_id,
                        ..Default::default()
                    },
                    &mut requests,
                    &mut next_id,
                    &mut stdin,
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

fn send_request<P: Serialize>(
    method: &'static str,
    params: P,
    requests: &mut PendingRequests,
    next_id: &mut i64,
    stdin: &mut impl Write,
) -> std::io::Result<()> {
    let id = *next_id;
    *next_id += 1;
    requests.insert(id, method.to_string());
    let params = serde_json::to_value(params).map_err(std::io::Error::other)?;
    let request = JSONRPCRequest {
        id: RequestId::Integer(id),
        method: method.into(),
        params: Some(params),
        trace: None,
    };
    let request = serde_json::to_string(&request).map_err(std::io::Error::other)?;
    writeln!(stdin, "{request}")?;
    stdin.flush()
}

fn handle_server_line(line: &str, event_tx: &Sender<BridgeEvent>, requests: &mut PendingRequests) {
    let parsed: JSONRPCMessage = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(err) => {
            let _ = event_tx.send(BridgeEvent::Error(format!(
                "Invalid app-server JSON: {err}: {line}"
            )));
            return;
        }
    };

    match parsed {
        JSONRPCMessage::Response(response) => {
            let method = request_id_i64(&response.id)
                .and_then(|id| requests.remove(id))
                .unwrap_or_default();
            handle_response(&method, response, event_tx);
        }
        JSONRPCMessage::Error(error) => handle_rpc_error(error, event_tx),
        JSONRPCMessage::Notification(notification) => handle_notification(notification, event_tx),
        JSONRPCMessage::Request(_) => {}
    }
}

fn handle_response(method: &str, response: JSONRPCResponse, event_tx: &Sender<BridgeEvent>) {
    match method {
        "initialize" => {
            let Ok(result) = decode_result::<InitializeResponse>(response.result, event_tx) else {
                return;
            };
            let user_agent = result.user_agent;
            let _ = event_tx.send(BridgeEvent::Status(format!("connected: {user_agent}")));
        }
        "thread/list" => {
            let Ok(result) = decode_result::<ThreadListResponse>(response.result, event_tx) else {
                return;
            };
            let _ = event_tx.send(BridgeEvent::ThreadsLoaded(result.data));
        }
        "thread/start" => {
            let Ok(result) = decode_result::<ThreadStartResponse>(response.result, event_tx) else {
                return;
            };
            let _ = event_tx.send(BridgeEvent::ThreadStarted(result.thread));
        }
        "thread/resume" => {
            let Ok(result) = decode_result::<ThreadResumeResponse>(response.result, event_tx)
            else {
                return;
            };
            let _ = event_tx.send(BridgeEvent::ThreadResumed(result.thread));
        }
        "thread/fork" => {
            let Ok(result) = decode_result::<ThreadForkResponse>(response.result, event_tx) else {
                return;
            };
            let _ = event_tx.send(BridgeEvent::ThreadForked(result.thread));
        }
        "turn/start" => {
            let _ = decode_result::<TurnStartResponse>(response.result, event_tx);
        }
        _ => {}
    }
}

fn handle_notification(notification: JSONRPCNotification, event_tx: &Sender<BridgeEvent>) {
    let notification = match ServerNotification::try_from(notification) {
        Ok(notification) => notification,
        Err(err) => {
            let _ = event_tx.send(BridgeEvent::Error(format!(
                "Invalid app-server notification: {err}"
            )));
            return;
        }
    };

    match notification {
        ServerNotification::ThreadStarted(params) => {
            let _ = event_tx.send(BridgeEvent::ThreadStarted(params.thread));
        }
        ServerNotification::TurnStarted(params) => {
            let _ = event_tx.send(BridgeEvent::TurnStarted {
                thread_id: params.thread_id,
            });
        }
        ServerNotification::ItemStarted(params) => {
            let _ = event_tx.send(BridgeEvent::ItemStarted {
                thread_id: params.thread_id,
                item: params.item,
            });
        }
        ServerNotification::AgentMessageDelta(params) => {
            let _ = event_tx.send(BridgeEvent::AgentMessageDelta {
                thread_id: params.thread_id,
                item_id: params.item_id,
                delta: params.delta,
            });
        }
        ServerNotification::CommandExecutionOutputDelta(params) => {
            let _ = event_tx.send(BridgeEvent::ToolOutputDelta {
                thread_id: params.thread_id,
                item_id: params.item_id,
                delta: params.delta,
            });
        }
        ServerNotification::ItemCompleted(params) => {
            let _ = event_tx.send(BridgeEvent::ItemCompleted {
                thread_id: params.thread_id,
                item: params.item,
            });
        }
        ServerNotification::ThreadStatusChanged(params) => {
            let _ = event_tx.send(BridgeEvent::Status(format!(
                "thread {}",
                thread_status_label(&params.status)
            )));
        }
        ServerNotification::TurnCompleted(_) => {
            let _ = event_tx.send(BridgeEvent::Status("turn complete".into()));
        }
        ServerNotification::Error(params) => {
            let _ = event_tx.send(BridgeEvent::Error(params.error.message));
        }
        _ => {}
    }
}

fn request_id_i64(id: &RequestId) -> Option<i64> {
    match id {
        RequestId::Integer(id) => Some(*id),
        RequestId::String(_) => None,
    }
}

fn handle_rpc_error(error: JSONRPCError, event_tx: &Sender<BridgeEvent>) {
    let _ = event_tx.send(BridgeEvent::Error(format!(
        "app-server error: {}",
        error.error.message
    )));
}

fn decode_result<T: DeserializeOwned>(
    result: serde_json::Value,
    event_tx: &Sender<BridgeEvent>,
) -> Result<T, ()> {
    serde_json::from_value(result).map_err(|err| {
        let _ = event_tx.send(BridgeEvent::Error(format!(
            "Invalid app-server response: {err}"
        )));
    })
}

fn thread_status_label(status: &ThreadStatus) -> &'static str {
    match status {
        ThreadStatus::NotLoaded => "not loaded",
        ThreadStatus::Idle => "idle",
        ThreadStatus::SystemError => "system error",
        ThreadStatus::Active { .. } => "active",
    }
}
