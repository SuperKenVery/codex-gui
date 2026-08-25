use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;

use codex_code_mode_protocol::CellId;
use codex_code_mode_protocol::CodeModeSession;
use codex_code_mode_protocol::CodeModeSessionCellExecutionLimits;
use codex_code_mode_protocol::CodeModeSessionDelegate;
use codex_code_mode_protocol::CodeModeSessionResultFuture;
use codex_code_mode_protocol::DEFAULT_EXEC_YIELD_TIME_MS;
use codex_code_mode_protocol::ExecuteRequest;
use codex_code_mode_protocol::NoopCodeModeSessionDelegate;
use codex_code_mode_protocol::RuntimeResponse;
use codex_code_mode_protocol::StartedCell;
use codex_code_mode_protocol::WaitOutcome;
use codex_code_mode_protocol::WaitRequest;
use serde_json::Value as JsonValue;
use tokio::sync::Mutex;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::bridge::RuntimeBridge;
use crate::cell::Cell;
use crate::quickjs::execute_javascript;

const YIELD_GRACE_PERIOD: Duration = Duration::from_secs(1);
const MIN_YIELD_TIME_FOR_GRACE: Duration = Duration::from_secs(10);

type TaskFailureHandler = Arc<dyn Fn(String) + Send + Sync>;

/// Drop-in QuickJS implementation of the runtime API consumed by
/// `codex-code-mode-host`.
pub struct InProcessCodeModeSession {
    inner: Arc<SessionInner>,
}

struct SessionInner {
    delegate: Arc<dyn CodeModeSessionDelegate>,
    stored_values: Mutex<HashMap<String, JsonValue>>,
    cells: Mutex<HashMap<CellId, Arc<Cell>>>,
    next_cell_id: AtomicU64,
    shutdown: CancellationToken,
    limits: CodeModeSessionCellExecutionLimits,
    task_failure_handler: Option<TaskFailureHandler>,
}

impl InProcessCodeModeSession {
    pub fn new() -> Self {
        Self::with_delegate(Arc::new(NoopCodeModeSessionDelegate))
    }

    pub fn with_delegate(delegate: Arc<dyn CodeModeSessionDelegate>) -> Self {
        Self::with_delegate_and_limits(delegate, CodeModeSessionCellExecutionLimits::default())
    }

    pub fn with_delegate_and_limits(
        delegate: Arc<dyn CodeModeSessionDelegate>,
        limits: CodeModeSessionCellExecutionLimits,
    ) -> Self {
        Self::from_parts(delegate, None, limits)
    }

    pub fn with_delegate_and_task_failure_handler(
        delegate: Arc<dyn CodeModeSessionDelegate>,
        task_failure_handler: TaskFailureHandler,
        limits: CodeModeSessionCellExecutionLimits,
    ) -> Self {
        Self::from_parts(delegate, Some(task_failure_handler), limits)
    }

    fn from_parts(
        delegate: Arc<dyn CodeModeSessionDelegate>,
        task_failure_handler: Option<TaskFailureHandler>,
        limits: CodeModeSessionCellExecutionLimits,
    ) -> Self {
        Self {
            inner: Arc::new(SessionInner {
                delegate,
                stored_values: Mutex::new(HashMap::new()),
                cells: Mutex::new(HashMap::new()),
                next_cell_id: AtomicU64::new(1),
                shutdown: CancellationToken::new(),
                limits,
                task_failure_handler,
            }),
        }
    }

    pub async fn execute(&self, request: ExecuteRequest) -> Result<StartedCell, String> {
        if self.inner.shutdown.is_cancelled() {
            return Err("code mode session is shutting down".to_string());
        }

        let numeric_id = self
            .inner
            .next_cell_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| "code mode session exhausted its cell ID space".to_string())?;
        let cell_id = CellId::new(numeric_id.to_string());
        let cell = Arc::new(Cell::new(
            cell_id.clone(),
            self.inner.shutdown.child_token(),
        ));
        self.inner
            .cells
            .lock()
            .await
            .insert(cell_id.clone(), Arc::clone(&cell));

        let stored_values = self.inner.stored_values.lock().await.clone();
        spawn_quickjs_cell(
            Arc::clone(&self.inner),
            Arc::clone(&cell),
            request.clone(),
            stored_values,
        );

        let yield_time_ms = request.yield_time_ms.unwrap_or(DEFAULT_EXEC_YIELD_TIME_MS);
        let timeout = self.inner.resolve_yield_timeout(yield_time_ms);
        let observer_inner = Arc::clone(&self.inner);
        let observer_cell = Arc::clone(&cell);
        let (response_tx, response_rx) = oneshot::channel();
        tokio::spawn(async move {
            let response = observer_inner.observe(observer_cell, timeout).await;
            let _ = response_tx.send(response);
        });

        Ok(StartedCell::from_result_receiver(cell_id, response_rx))
    }

    pub async fn wait(&self, request: WaitRequest) -> Result<WaitOutcome, String> {
        let cell = self.inner.cells.lock().await.get(&request.cell_id).cloned();
        let Some(cell) = cell else {
            return Ok(WaitOutcome::MissingCell(missing_cell_response(
                request.cell_id,
            )));
        };
        let timeout = self.inner.resolve_yield_timeout(request.yield_time_ms);
        self.inner
            .observe(cell, timeout)
            .await
            .map(WaitOutcome::LiveCell)
    }

    pub async fn terminate(&self, cell_id: CellId) -> Result<WaitOutcome, String> {
        let cell = self.inner.cells.lock().await.get(&cell_id).cloned();
        let Some(cell) = cell else {
            return Ok(WaitOutcome::MissingCell(missing_cell_response(cell_id)));
        };
        cell.cancellation.cancel();
        self.inner
            .observe_terminal(cell)
            .await
            .map(WaitOutcome::LiveCell)
    }

    pub async fn shutdown(&self) -> Result<(), String> {
        self.inner.shutdown.cancel();
        let cells = self
            .inner
            .cells
            .lock()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for cell in cells {
            cell.cancellation.cancel();
            cell.wait_until_terminal().await;
            self.inner.finish_cell(&cell).await;
        }
        Ok(())
    }
}

impl Default for InProcessCodeModeSession {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeModeSession for InProcessCodeModeSession {
    fn execute<'a>(
        &'a self,
        request: ExecuteRequest,
    ) -> CodeModeSessionResultFuture<'a, StartedCell> {
        Box::pin(InProcessCodeModeSession::execute(self, request))
    }

    fn wait<'a>(&'a self, request: WaitRequest) -> CodeModeSessionResultFuture<'a, WaitOutcome> {
        Box::pin(InProcessCodeModeSession::wait(self, request))
    }

    fn terminate<'a>(&'a self, cell_id: CellId) -> CodeModeSessionResultFuture<'a, WaitOutcome> {
        Box::pin(InProcessCodeModeSession::terminate(self, cell_id))
    }

    fn shutdown<'a>(&'a self) -> CodeModeSessionResultFuture<'a, ()> {
        Box::pin(InProcessCodeModeSession::shutdown(self))
    }
}

impl SessionInner {
    fn resolve_yield_timeout(&self, yield_time_ms: u64) -> Duration {
        let yield_time = Duration::from_millis(yield_time_ms);
        let timeout = if yield_time >= MIN_YIELD_TIME_FOR_GRACE {
            yield_time.saturating_add(YIELD_GRACE_PERIOD)
        } else {
            yield_time
        };
        self.limits
            .max_yield_time_ms
            .map(Duration::from_millis)
            .map_or(timeout, |limit| timeout.min(limit))
    }

    async fn observe(
        self: &Arc<Self>,
        cell: Arc<Cell>,
        timeout: Duration,
    ) -> Result<RuntimeResponse, String> {
        let _observer = cell
            .observer
            .try_lock()
            .map_err(|_| format!("exec cell {} already has an active observer", cell.id))?;
        let sleep = tokio::time::sleep(timeout);
        tokio::pin!(sleep);

        loop {
            let notified = cell.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if let Some(response) = cell.take_observable(false) {
                if is_terminal(&response) {
                    self.finish_cell(&cell).await;
                }
                return Ok(response);
            }
            tokio::select! {
                _ = &mut sleep => {
                    if let Some(response) = cell.take_observable(false) {
                        if is_terminal(&response) {
                            self.finish_cell(&cell).await;
                        }
                        return Ok(response);
                    }
                    return Ok(cell.take_yielded());
                }
                _ = &mut notified => {}
            }
        }
    }

    async fn observe_terminal(
        self: &Arc<Self>,
        cell: Arc<Cell>,
    ) -> Result<RuntimeResponse, String> {
        let _observer = cell
            .observer
            .try_lock()
            .map_err(|_| format!("exec cell {} already has an active observer", cell.id))?;
        loop {
            let notified = cell.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if let Some(response) = cell.take_observable(true) {
                if is_terminal(&response) {
                    self.finish_cell(&cell).await;
                    return Ok(response);
                }
            }
            notified.await;
        }
    }

    async fn finish_cell(&self, cell: &Arc<Cell>) {
        if cell.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        self.cells.lock().await.remove(&cell.id);
        self.delegate.cell_closed(&cell.id);
    }
}

fn spawn_quickjs_cell(
    inner: Arc<SessionInner>,
    cell: Arc<Cell>,
    request: ExecuteRequest,
    stored_values: HashMap<String, JsonValue>,
) {
    let failure_handler = inner.task_failure_handler.clone();
    let runtime_cell = Arc::clone(&cell);
    std::thread::Builder::new()
        .name(format!("codex-quickjs-cell-{}", cell.id))
        .spawn(move || {
            let thread_result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| format!("failed to start QuickJS runtime: {error}"))?;
                runtime.block_on(run_quickjs_cell(
                    Arc::clone(&inner),
                    Arc::clone(&runtime_cell),
                    request,
                    stored_values,
                ))
            }));
            match thread_result {
                Ok(Ok(())) => {}
                Ok(Err(error_text)) => runtime_cell.complete(Some(error_text)),
                Err(_) => {
                    let reason =
                        format!("code-mode QuickJS cell {} thread panicked", runtime_cell.id);
                    if let Some(handler) = failure_handler {
                        handler(reason.clone());
                    }
                    runtime_cell.complete(Some(reason));
                }
            }
        })
        .map_err(|error| cell.complete(Some(format!("failed to spawn QuickJS runtime: {error}"))))
        .ok();
}

async fn run_quickjs_cell(
    inner: Arc<SessionInner>,
    cell: Arc<Cell>,
    request: ExecuteRequest,
    stored_values: HashMap<String, JsonValue>,
) -> Result<(), String> {
    let bridge = Arc::new(RuntimeBridge::new(
        Arc::clone(&inner.delegate),
        Arc::clone(&cell),
        request.tool_call_id.clone(),
        request.enabled_tools.clone(),
        stored_values,
    ));
    let error_text = execute_javascript(
        request.source,
        Arc::clone(&bridge),
        inner.limits.max_heap_size_bytes,
    )
    .await
    .err();
    bridge.wait_for_notifications().await;

    if error_text.is_none() && !cell.cancellation.is_cancelled() {
        inner
            .stored_values
            .lock()
            .await
            .extend(bridge.stored_value_writes());
    }
    cell.complete(error_text);
    Ok(())
}

fn is_terminal(response: &RuntimeResponse) -> bool {
    matches!(
        response,
        RuntimeResponse::Result { .. } | RuntimeResponse::Terminated { .. }
    )
}

fn missing_cell_response(cell_id: CellId) -> RuntimeResponse {
    RuntimeResponse::Result {
        error_text: Some(format!("exec cell {cell_id} not found")),
        cell_id,
        content_items: Vec::new(),
    }
}
