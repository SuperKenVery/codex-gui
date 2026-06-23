use crate::gui::{
    ApprovalReviewerMode, ChatSettings, ModelOption, PermissionMode, PermissionProfileOption,
    permission_profile_label,
};
use crate::workspace::workspace_path;
use codex_app_server_protocol::{
    ApprovalsReviewer, AskForApproval, ClientInfo, InitializeCapabilities, InitializeParams,
    InitializeResponse, JSONRPCError, JSONRPCMessage, JSONRPCNotification, JSONRPCRequest,
    JSONRPCResponse, ModelListParams, ModelListResponse, PermissionProfileListParams,
    PermissionProfileListResponse, RequestId, ServerNotification, Thread, ThreadForkParams,
    ThreadForkResponse, ThreadItem, ThreadListCwdFilter, ThreadListParams, ThreadListResponse,
    ThreadResumeParams, ThreadResumeResponse, ThreadSource, ThreadStartParams, ThreadStartResponse,
    ThreadStatus, TurnInterruptParams, TurnStartParams, TurnStartResponse, TurnSteerParams,
    TurnSteerResponse, UserInput,
};
use codex_protocol::openai_models::ReasoningEffort;
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
    ListThreads {
        cwd: String,
    },
    StartThread {
        cwd: String,
        settings: ChatSettings,
    },
    ResumeThread {
        thread_id: String,
    },
    SendTurn {
        thread_id: String,
        text: String,
        settings: ChatSettings,
    },
    SteerTurn {
        thread_id: String,
        turn_id: String,
        text: String,
    },
    InterruptTurn {
        thread_id: String,
        turn_id: String,
    },
    ForkThread {
        thread_id: String,
    },
    UpdateThreadSettings {
        thread_id: String,
        settings: ChatSettings,
    },
}

pub enum BridgeEvent {
    Status(String),
    ThreadsLoaded {
        cwd: String,
        threads: Vec<Thread>,
    },
    ModelsLoaded(Vec<ModelOption>),
    PermissionProfilesLoaded(Vec<PermissionProfileOption>),
    ThreadStarted(Thread),
    ThreadResumed(Thread),
    ThreadForked(Thread),
    ThreadNameUpdated {
        thread_id: String,
        thread_name: Option<String>,
    },
    TurnStarted {
        thread_id: String,
        turn_id: String,
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
    TurnCompleted {
        thread_id: String,
        turn_id: String,
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
            capabilities: Some(InitializeCapabilities {
                experimental_api: true,
                ..Default::default()
            }),
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
        "model/list",
        ModelListParams {
            cursor: None,
            limit: None,
            include_hidden: None,
        },
        &mut requests,
        &mut next_id,
        &mut stdin,
    ) {
        let _ = event_tx.send(BridgeEvent::Error(format!(
            "Failed to list codex models: {err}"
        )));
    }

    if let Err(err) = send_request(
        "permissionProfile/list",
        PermissionProfileListParams {
            cursor: None,
            limit: None,
            cwd: Some(workspace_path()),
        },
        &mut requests,
        &mut next_id,
        &mut stdin,
    ) {
        let _ = event_tx.send(BridgeEvent::Error(format!(
            "Failed to list codex permission profiles: {err}"
        )));
    }

    if let Err(err) =
        send_thread_list_request(workspace_path(), &mut requests, &mut next_id, &mut stdin)
    {
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
                BridgeCommand::ListThreads { cwd } => {
                    send_thread_list_request(cwd, &mut requests, &mut next_id, &mut stdin)
                }
                BridgeCommand::StartThread { cwd, settings } => send_request(
                    "thread/start",
                    ThreadStartParams {
                        cwd: Some(cwd),
                        model: Some(settings.model.clone()),
                        approval_policy: Some(approval_policy_for(&settings)),
                        approvals_reviewer: Some(approvals_reviewer_for(&settings)),
                        permissions: Some(settings.permission_profile.clone()),
                        sandbox: None,
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
                BridgeCommand::SendTurn {
                    thread_id,
                    text,
                    settings,
                } => send_request(
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
                        approval_policy: Some(approval_policy_for(&settings)),
                        approvals_reviewer: Some(approvals_reviewer_for(&settings)),
                        sandbox_policy: None,
                        permissions: Some(settings.permission_profile.clone()),
                        model: Some(settings.model.clone()),
                        service_tier: None,
                        effort: Some(reasoning_effort_for(&settings)),
                        summary: None,
                        personality: None,
                        output_schema: None,
                        collaboration_mode: None,
                    },
                    &mut requests,
                    &mut next_id,
                    &mut stdin,
                ),
                BridgeCommand::SteerTurn {
                    thread_id,
                    turn_id,
                    text,
                } => send_request(
                    "turn/steer",
                    TurnSteerParams {
                        thread_id,
                        client_user_message_id: None,
                        input: vec![UserInput::Text {
                            text,
                            text_elements: Vec::new(),
                        }],
                        responsesapi_client_metadata: None,
                        additional_context: None,
                        expected_turn_id: turn_id,
                    },
                    &mut requests,
                    &mut next_id,
                    &mut stdin,
                ),
                BridgeCommand::InterruptTurn { thread_id, turn_id } => send_request(
                    "turn/interrupt",
                    TurnInterruptParams { thread_id, turn_id },
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
                BridgeCommand::UpdateThreadSettings {
                    thread_id,
                    settings,
                } => send_request(
                    "thread/settings/update",
                    ThreadSettingsUpdateParamsJson {
                        thread_id,
                        approval_policy: Some(approval_policy_for(&settings)),
                        approvals_reviewer: Some(approvals_reviewer_for(&settings)),
                        permissions: Some(settings.permission_profile),
                        model: Some(settings.model),
                        effort: Some(settings.effort),
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

fn send_thread_list_request(
    cwd: String,
    requests: &mut PendingRequests,
    next_id: &mut i64,
    stdin: &mut impl Write,
) -> std::io::Result<()> {
    let method = format!("thread/list:{cwd}");
    let id = *next_id;
    *next_id += 1;
    requests.insert(id, method);
    let params = serde_json::to_value(ThreadListParams {
        cursor: None,
        limit: Some(30),
        sort_key: None,
        sort_direction: None,
        model_providers: None,
        source_kinds: None,
        archived: Some(false),
        cwd: Some(ThreadListCwdFilter::One(cwd)),
        use_state_db_only: false,
        search_term: None,
    })
    .map_err(std::io::Error::other)?;
    let request = JSONRPCRequest {
        id: RequestId::Integer(id),
        method: "thread/list".into(),
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
        method if method.starts_with("thread/list:") => {
            let Ok(result) = decode_result::<ThreadListResponse>(response.result, event_tx) else {
                return;
            };
            let cwd = method.trim_start_matches("thread/list:").to_string();
            let _ = event_tx.send(BridgeEvent::ThreadsLoaded {
                cwd,
                threads: result.data,
            });
        }
        "model/list" => {
            let Ok(result) = decode_result::<ModelListResponse>(response.result, event_tx) else {
                return;
            };
            let models = result
                .data
                .into_iter()
                .filter(|model| !model.hidden)
                .map(|model| ModelOption {
                    id: model.model,
                    display_name: model.display_name,
                    supported_efforts: model
                        .supported_reasoning_efforts
                        .into_iter()
                        .map(|effort| effort.reasoning_effort.to_string())
                        .collect(),
                    default_effort: model.default_reasoning_effort.to_string(),
                })
                .collect();
            let _ = event_tx.send(BridgeEvent::ModelsLoaded(models));
        }
        "permissionProfile/list" => {
            let Ok(result) =
                decode_result::<PermissionProfileListResponse>(response.result, event_tx)
            else {
                return;
            };
            let profiles = result
                .data
                .into_iter()
                .map(|profile| PermissionProfileOption {
                    label: permission_profile_label(&profile.id),
                    id: profile.id,
                    description: profile.description,
                })
                .collect();
            let _ = event_tx.send(BridgeEvent::PermissionProfilesLoaded(profiles));
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
        "turn/steer" => {
            let _ = decode_result::<TurnSteerResponse>(response.result, event_tx);
        }
        "turn/interrupt" => {}
        "thread/settings/update" => {}
        _ => {}
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ThreadSettingsUpdateParamsJson {
    thread_id: String,
    approval_policy: Option<AskForApproval>,
    approvals_reviewer: Option<ApprovalsReviewer>,
    permissions: Option<String>,
    model: Option<String>,
    effort: Option<String>,
}

fn approval_policy_for(settings: &ChatSettings) -> AskForApproval {
    if settings.permission_profile == PermissionMode::DangerFullAccess.profile_id() {
        AskForApproval::Never
    } else {
        AskForApproval::OnRequest
    }
}

fn reasoning_effort_for(settings: &ChatSettings) -> ReasoningEffort {
    settings
        .effort
        .parse()
        .unwrap_or_else(|_| ReasoningEffort::Medium)
}

fn approvals_reviewer_for(settings: &ChatSettings) -> ApprovalsReviewer {
    match settings.approvals_reviewer {
        ApprovalReviewerMode::User => ApprovalsReviewer::User,
        ApprovalReviewerMode::AutoReview => ApprovalsReviewer::AutoReview,
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
        ServerNotification::ThreadNameUpdated(params) => {
            let _ = event_tx.send(BridgeEvent::ThreadNameUpdated {
                thread_id: params.thread_id,
                thread_name: params.thread_name,
            });
        }
        ServerNotification::TurnStarted(params) => {
            let _ = event_tx.send(BridgeEvent::TurnStarted {
                thread_id: params.thread_id,
                turn_id: params.turn.id,
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
        ServerNotification::TurnCompleted(params) => {
            let _ = event_tx.send(BridgeEvent::Status("turn complete".into()));
            let _ = event_tx.send(BridgeEvent::TurnCompleted {
                thread_id: params.thread_id,
                turn_id: params.turn.id,
            });
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
