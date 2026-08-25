use std::sync::Mutex as StdMutex;
use std::sync::PoisonError;
use std::sync::atomic::AtomicBool;

use codex_code_mode_protocol::CellId;
use codex_code_mode_protocol::FunctionCallOutputContentItem;
use codex_code_mode_protocol::RuntimeResponse;
use tokio::sync::Mutex;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

pub(crate) struct Cell {
    pub(crate) id: CellId,
    state: StdMutex<CellState>,
    pub(crate) changed: Notify,
    pub(crate) observer: Mutex<()>,
    pub(crate) cancellation: CancellationToken,
    pub(crate) closed: AtomicBool,
}

struct CellState {
    output: Vec<FunctionCallOutputContentItem>,
    status: CellStatus,
    yield_generation: u64,
    observed_yield_generation: u64,
}

enum CellStatus {
    Running,
    Completed { error_text: Option<String> },
    Terminated,
}

impl Cell {
    pub(crate) fn new(id: CellId, cancellation: CancellationToken) -> Self {
        Self {
            id,
            state: StdMutex::new(CellState {
                output: Vec::new(),
                status: CellStatus::Running,
                yield_generation: 0,
                observed_yield_generation: 0,
            }),
            changed: Notify::new(),
            observer: Mutex::new(()),
            cancellation,
            closed: AtomicBool::new(false),
        }
    }

    pub(crate) fn push(&self, item: FunctionCallOutputContentItem) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if matches!(state.status, CellStatus::Running) {
            state.output.push(item);
        }
    }

    pub(crate) fn request_yield(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if matches!(state.status, CellStatus::Running) {
            state.yield_generation = state.yield_generation.saturating_add(1);
            drop(state);
            self.changed.notify_waiters();
        }
    }

    pub(crate) fn complete(&self, error_text: Option<String>) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if matches!(state.status, CellStatus::Running) {
            state.status = if self.cancellation.is_cancelled() {
                CellStatus::Terminated
            } else {
                CellStatus::Completed { error_text }
            };
            drop(state);
            self.changed.notify_waiters();
        }
    }

    pub(crate) fn take_observable(&self, terminal_only: bool) -> Option<RuntimeResponse> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let cell_id = self.id.clone();
        let status = match &state.status {
            CellStatus::Running if !terminal_only => {
                if state.yield_generation > state.observed_yield_generation {
                    state.observed_yield_generation = state.yield_generation;
                    let content_items = std::mem::take(&mut state.output);
                    return Some(RuntimeResponse::Yielded {
                        cell_id,
                        content_items,
                    });
                }
                return None;
            }
            CellStatus::Running => return None,
            CellStatus::Completed { error_text } => Some(error_text.clone()),
            CellStatus::Terminated => None,
        };
        let content_items = std::mem::take(&mut state.output);
        Some(match &state.status {
            CellStatus::Completed { .. } => RuntimeResponse::Result {
                cell_id,
                content_items,
                error_text: status.flatten(),
            },
            CellStatus::Terminated => RuntimeResponse::Terminated {
                cell_id,
                content_items,
            },
            CellStatus::Running => unreachable!(),
        })
    }

    pub(crate) fn take_yielded(&self) -> RuntimeResponse {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        RuntimeResponse::Yielded {
            cell_id: self.id.clone(),
            content_items: std::mem::take(&mut state.output),
        }
    }

    pub(crate) async fn wait_until_terminal(&self) {
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if !matches!(
                self.state
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .status,
                CellStatus::Running
            ) {
                return;
            }
            notified.await;
        }
    }
}
