use super::CodexGui;
use crate::gui::{PendingApproval, PendingApprovalKind, PendingUserInputRequest};
use codex_app_server_protocol::{
    GrantedPermissionProfile, McpServerElicitationRequest, ServerRequest,
};
use gpui::Context;

impl CodexGui {
    pub(super) fn apply_server_request(&mut self, request: ServerRequest, cx: &mut Context<Self>) {
        let (thread_id, approval) = match request {
            ServerRequest::CommandExecutionRequestApproval { request_id, params } => {
                let mut details = Vec::new();
                if let Some(command) = params.command {
                    details.push(command);
                }
                if let Some(cwd) = params.cwd {
                    details.push(format!("Working directory: {cwd}"));
                }
                if let Some(reason) = params.reason {
                    details.push(reason);
                }
                (
                    params.thread_id,
                    PendingApproval {
                        request_id,
                        title: "Allow command?".into(),
                        body: details.join("\n").into(),
                        kind: PendingApprovalKind::Command,
                    },
                )
            }
            ServerRequest::FileChangeRequestApproval { request_id, params } => {
                let mut details = Vec::new();
                if let Some(reason) = params.reason {
                    details.push(reason);
                }
                if let Some(root) = params.grant_root {
                    details.push(format!("Requested write root: {}", root.display()));
                }
                (
                    params.thread_id,
                    PendingApproval {
                        request_id,
                        title: "Allow file changes?".into(),
                        body: details.join("\n").into(),
                        kind: PendingApprovalKind::FileChange,
                    },
                )
            }
            ServerRequest::PermissionsRequestApproval { request_id, params } => {
                let permissions = GrantedPermissionProfile {
                    network: params.permissions.network.clone(),
                    file_system: params.permissions.file_system.clone(),
                };
                let mut details = Vec::new();
                if let Some(reason) = params.reason {
                    details.push(reason);
                }
                details.push(format!("Working directory: {}", params.cwd.display()));
                if let Ok(serialized) = serde_json::to_string_pretty(&params.permissions) {
                    details.push(serialized);
                }
                (
                    params.thread_id,
                    PendingApproval {
                        request_id,
                        title: "Allow additional permissions?".into(),
                        body: details.join("\n").into(),
                        kind: PendingApprovalKind::Permissions { permissions },
                    },
                )
            }
            ServerRequest::ToolRequestUserInput { request_id, params } => {
                let thread_id = params.thread_id;
                self.update_chat(&thread_id, cx, |chat| {
                    chat.upsert_input_request(PendingUserInputRequest {
                        request_id,
                        questions: params.questions,
                        answers: Default::default(),
                    })
                });
                return;
            }
            ServerRequest::McpServerElicitationRequest { request_id, params } => {
                let (message, details, accept_content, meta) = match params.request {
                    McpServerElicitationRequest::Form {
                        message,
                        requested_schema,
                        meta,
                    } => (
                        message,
                        serde_json::to_string_pretty(&requested_schema).unwrap_or_default(),
                        Some(serde_json::json!({})),
                        meta,
                    ),
                    McpServerElicitationRequest::OpenAiForm {
                        message,
                        requested_schema,
                        meta,
                    } => (
                        message,
                        serde_json::to_string_pretty(&requested_schema).unwrap_or_default(),
                        Some(serde_json::json!({})),
                        meta,
                    ),
                    McpServerElicitationRequest::Url {
                        message, url, meta, ..
                    } => (message, url, None, meta),
                };
                (
                    params.thread_id,
                    PendingApproval {
                        request_id,
                        title: format!("Allow input requested by {}?", params.server_name).into(),
                        body: format!("{message}\n{details}").into(),
                        kind: PendingApprovalKind::McpElicitation {
                            accept_content,
                            meta,
                        },
                    },
                )
            }
            unsupported => {
                let request_id = unsupported.id().clone();
                let message =
                    format!("Unsupported interactive app-server request: {unsupported:?}");
                tracing::warn!(%request_id, %message);
                let bridge = self.bridge.clone();
                cx.spawn(async move |this, cx| {
                    let result = bridge.reject_server_request(request_id, message).await;
                    if let Err(error) = result {
                        let _ = this.update(cx, |view, cx| {
                            view.apply_bridge_error(error.to_string(), cx)
                        });
                    }
                })
                .detach();
                return;
            }
        };

        self.update_chat(&thread_id, cx, |chat| chat.upsert_approval(approval));
    }
}
