use crate::gui::{
    ApprovalReviewerMode, ChatSettings, ModelOption, PermissionMode, PermissionProfileOption,
    permission_profile_label,
};
use codex_app_server_protocol::{
    ApprovalsReviewer, AskForApproval, ClientInfo, InitializeCapabilities, InitializeParams,
    InitializeResponse, JSONRPCError, JSONRPCMessage, JSONRPCNotification, JSONRPCRequest,
    ModelListParams, ModelListResponse, PermissionProfileListParams, PermissionProfileListResponse,
    RequestId, ServerNotification, Thread, ThreadForkParams, ThreadForkResponse,
    ThreadListCwdFilter, ThreadListParams, ThreadListResponse, ThreadResumeParams,
    ThreadResumeResponse, ThreadSource, ThreadStartParams, ThreadStartResponse,
    TurnInterruptParams, TurnStartParams, TurnStartResponse, TurnSteerParams, TurnSteerResponse,
    UserInput,
};
use codex_protocol::openai_models::ReasoningEffort;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::{
    collections::HashMap,
    fmt,
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::Duration,
};

#[derive(Clone)]
pub struct AppServerBridge {
    request_tx: Sender<BridgeRequest>,
}

pub enum BridgeEvent {
    Notification(ServerNotification),
    RpcError(JSONRPCError),
    TransportError(String),
    Stderr(String),
}

#[derive(Debug)]
pub enum BridgeError {
    Transport(String),
    Rpc(String),
    Decode(String),
}

impl fmt::Display for BridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(message) | Self::Rpc(message) | Self::Decode(message) => {
                f.write_str(message)
            }
        }
    }
}

impl std::error::Error for BridgeError {}

type BridgeResult<T> = Result<T, BridgeError>;
type ResponseResult = Result<serde_json::Value, BridgeError>;

struct BridgeRequest {
    method: &'static str,
    params: serde_json::Value,
    response_tx: Sender<ResponseResult>,
}

pub fn start_app_server_bridge() -> (AppServerBridge, Receiver<BridgeEvent>) {
    let (request_tx, request_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::channel();

    thread::spawn(move || run_app_server_bridge(request_rx, event_tx));

    (AppServerBridge { request_tx }, event_rx)
}

impl AppServerBridge {
    pub async fn initialize(&self) -> BridgeResult<InitializeResponse> {
        self.request(
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
        )
        .await
    }

    pub async fn list_threads(&self, cwd: String) -> BridgeResult<Vec<Thread>> {
        let response: ThreadListResponse = self
            .request(
                "thread/list",
                ThreadListParams {
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
                },
            )
            .await?;
        Ok(response.data)
    }

    pub async fn list_models(&self) -> BridgeResult<Vec<ModelOption>> {
        let response: ModelListResponse = self
            .request(
                "model/list",
                ModelListParams {
                    cursor: None,
                    limit: None,
                    include_hidden: None,
                },
            )
            .await?;
        Ok(response
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
            .collect())
    }

    pub async fn list_permission_profiles(
        &self,
        cwd: String,
    ) -> BridgeResult<Vec<PermissionProfileOption>> {
        let response: PermissionProfileListResponse = self
            .request(
                "permissionProfile/list",
                PermissionProfileListParams {
                    cursor: None,
                    limit: None,
                    cwd: Some(cwd),
                },
            )
            .await?;
        Ok(response
            .data
            .into_iter()
            .map(|profile| PermissionProfileOption {
                label: permission_profile_label(&profile.id),
                id: profile.id,
                description: profile.description,
            })
            .collect())
    }

    pub async fn start_thread(&self, cwd: String, settings: ChatSettings) -> BridgeResult<Thread> {
        let response: ThreadStartResponse = self
            .request(
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
            )
            .await?;
        Ok(response.thread)
    }

    pub async fn resume_thread(&self, thread_id: String) -> BridgeResult<Thread> {
        let response: ThreadResumeResponse = self
            .request(
                "thread/resume",
                ThreadResumeParams {
                    thread_id,
                    ..Default::default()
                },
            )
            .await?;
        Ok(response.thread)
    }

    pub async fn fork_thread(&self, thread_id: String) -> BridgeResult<Thread> {
        let response: ThreadForkResponse = self
            .request(
                "thread/fork",
                ThreadForkParams {
                    thread_id,
                    ..Default::default()
                },
            )
            .await?;
        Ok(response.thread)
    }

    pub async fn send_turn(
        &self,
        thread_id: String,
        text: String,
        settings: ChatSettings,
    ) -> BridgeResult<TurnStartResponse> {
        self.request(
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
        )
        .await
    }

    pub async fn steer_turn(
        &self,
        thread_id: String,
        turn_id: String,
        text: String,
    ) -> BridgeResult<TurnSteerResponse> {
        self.request(
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
        )
        .await
    }

    pub async fn interrupt_turn(&self, thread_id: String, turn_id: String) -> BridgeResult<()> {
        self.request_value("turn/interrupt", TurnInterruptParams { thread_id, turn_id })
            .await
            .map(|_| ())
    }

    pub async fn update_thread_settings(
        &self,
        thread_id: String,
        settings: ChatSettings,
    ) -> BridgeResult<()> {
        self.request_value(
            "thread/settings/update",
            ThreadSettingsUpdateParamsJson {
                thread_id,
                approval_policy: Some(approval_policy_for(&settings)),
                approvals_reviewer: Some(approvals_reviewer_for(&settings)),
                permissions: Some(settings.permission_profile),
                model: Some(settings.model),
                effort: Some(settings.effort),
            },
        )
        .await
        .map(|_| ())
    }

    async fn request<T, P>(&self, method: &'static str, params: P) -> BridgeResult<T>
    where
        T: DeserializeOwned,
        P: Serialize,
    {
        let value = self.request_value(method, params).await?;
        serde_json::from_value(value).map_err(|err| {
            BridgeError::Decode(format!("Invalid app-server response for {method}: {err}"))
        })
    }

    async fn request_value<P>(
        &self,
        method: &'static str,
        params: P,
    ) -> BridgeResult<serde_json::Value>
    where
        P: Serialize,
    {
        let params = serde_json::to_value(params)
            .map_err(|err| BridgeError::Decode(format!("Invalid request params: {err}")))?;
        let (response_tx, response_rx) = mpsc::channel();
        self.request_tx
            .send(BridgeRequest {
                method,
                params,
                response_tx,
            })
            .map_err(|_| BridgeError::Transport("codex app-server writer stopped".into()))?;
        response_rx.recv().map_err(|_| {
            BridgeError::Transport("codex app-server response channel closed".into())
        })?
    }
}

fn run_app_server_bridge(request_rx: Receiver<BridgeRequest>, event_tx: Sender<BridgeEvent>) {
    let mut child = match Command::new("codex")
        .args(["app-server", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => {
            let _ = event_tx.send(BridgeEvent::TransportError(format!(
                "Failed to start codex app-server: {err}"
            )));
            return;
        }
    };

    let Some(mut stdin) = child.stdin.take() else {
        let _ = event_tx.send(BridgeEvent::TransportError(
            "codex app-server stdin unavailable".into(),
        ));
        return;
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = event_tx.send(BridgeEvent::TransportError(
            "codex app-server stdout unavailable".into(),
        ));
        return;
    };

    if let Some(stderr) = child.stderr.take() {
        let event_tx = event_tx.clone();
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if !line.trim().is_empty() {
                    let _ = event_tx.send(BridgeEvent::Stderr(line));
                }
            }
        });
    }

    let (line_tx, line_rx) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = line_tx.send(line);
        }
    });

    let mut requests = PendingRequests::default();
    let mut next_id = 1_i64;

    loop {
        while let Ok(request) = request_rx.try_recv() {
            if let Err(err) = send_request(request, &mut requests, &mut next_id, &mut stdin) {
                let _ = err.response_tx.send(Err(BridgeError::Transport(format!(
                    "Failed to send codex app-server request: {}",
                    err.error
                ))));
            }
        }

        match line_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(line) => handle_server_line(&line, &event_tx, &mut requests),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let message = "codex app-server stdout stream closed".to_string();
                requests.close_all(&message);
                let _ = event_tx.send(BridgeEvent::TransportError(message));
                break;
            }
        }
    }

    let _ = child.kill();
}

#[derive(Default)]
struct PendingRequests(HashMap<i64, Sender<ResponseResult>>);

impl PendingRequests {
    fn insert(&mut self, id: i64, response_tx: Sender<ResponseResult>) {
        self.0.insert(id, response_tx);
    }

    fn remove(&mut self, id: i64) -> Option<Sender<ResponseResult>> {
        self.0.remove(&id)
    }

    fn close_all(&mut self, message: &str) {
        for (_, response_tx) in self.0.drain() {
            let _ = response_tx.send(Err(BridgeError::Transport(message.to_string())));
        }
    }
}

struct SendRequestError {
    response_tx: Sender<ResponseResult>,
    error: std::io::Error,
}

fn send_request(
    request: BridgeRequest,
    requests: &mut PendingRequests,
    next_id: &mut i64,
    stdin: &mut impl Write,
) -> Result<(), SendRequestError> {
    let id = *next_id;
    *next_id += 1;

    let rpc_request = JSONRPCRequest {
        id: RequestId::Integer(id),
        method: request.method.into(),
        params: Some(request.params),
        trace: None,
    };
    let serialized = serde_json::to_string(&rpc_request).map_err(|error| SendRequestError {
        response_tx: request.response_tx.clone(),
        error: std::io::Error::other(error),
    })?;

    if let Err(error) = writeln!(stdin, "{serialized}").and_then(|_| stdin.flush()) {
        return Err(SendRequestError {
            response_tx: request.response_tx,
            error,
        });
    }

    requests.insert(id, request.response_tx);
    Ok(())
}

fn handle_server_line(line: &str, event_tx: &Sender<BridgeEvent>, requests: &mut PendingRequests) {
    let parsed: JSONRPCMessage = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(err) => {
            let _ = event_tx.send(BridgeEvent::TransportError(format!(
                "Invalid app-server JSON: {err}: {line}"
            )));
            return;
        }
    };

    match parsed {
        JSONRPCMessage::Response(response) => {
            if let Some(response_tx) =
                request_id_i64(&response.id).and_then(|id| requests.remove(id))
            {
                let _ = response_tx.send(Ok(response.result));
            }
        }
        JSONRPCMessage::Error(error) => {
            if let Some(response_tx) = request_id_i64(&error.id).and_then(|id| requests.remove(id))
            {
                let _ = response_tx.send(Err(BridgeError::Rpc(format!(
                    "app-server error: {}",
                    error.error.message
                ))));
            }
            let _ = event_tx.send(BridgeEvent::RpcError(error));
        }
        JSONRPCMessage::Notification(notification) => handle_notification(notification, event_tx),
        JSONRPCMessage::Request(_) => {}
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
    match ServerNotification::try_from(notification) {
        Ok(notification) => {
            let _ = event_tx.send(BridgeEvent::Notification(notification));
        }
        Err(err) => {
            let _ = event_tx.send(BridgeEvent::TransportError(format!(
                "Invalid app-server notification: {err}"
            )));
        }
    }
}

fn request_id_i64(id: &RequestId) -> Option<i64> {
    match id {
        RequestId::Integer(id) => Some(*id),
        RequestId::String(_) => None,
    }
}
