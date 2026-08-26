use codex_app_server_protocol::{PatchChangeKind, ThreadItem};
use gpui::{App, IntoElement, RenderOnce, SharedString, Window};
use gpui_component::IconName;

use super::simple::{ToolFrame, ToolStatus, append_progress};

#[derive(Clone, IntoElement)]
pub(in crate::gui::chat_history) struct FileChangeTool {
    title: SharedString,
    detail: Option<SharedString>,
    status: ToolStatus,
    additions: usize,
    deletions: usize,
}

impl FileChangeTool {
    pub(super) fn new(
        item: &ThreadItem,
        status: ToolStatus,
        progress: Option<&[SharedString]>,
    ) -> Option<Self> {
        let ThreadItem::FileChange { changes, .. } = item else {
            return None;
        };
        if changes.is_empty() {
            return Some(Self {
                title: "Preparing file edits".into(),
                detail: append_progress(None, progress),
                status,
                additions: 0,
                deletions: 0,
            });
        }

        let (additions, deletions) = changes.iter().fold((0, 0), |mut total, change| {
            let stats = diff_stats(&change.diff);
            total.0 += stats.0;
            total.1 += stats.1;
            total
        });
        let action = change_action(&changes[0].kind, status);
        let all_same_action = changes
            .iter()
            .all(|change| change_action(&change.kind, status) == action);
        let title = if let [change] = changes.as_slice() {
            let path = match &change.kind {
                PatchChangeKind::Update {
                    move_path: Some(move_path),
                } => format!("{} → {}", change.path, move_path.display()),
                _ => change.path.clone(),
            };
            format!("{action} {path}")
        } else if all_same_action {
            format!("{action} {} files", changes.len())
        } else if matches!(status, ToolStatus::Running) {
            format!("Changing {} files", changes.len())
        } else {
            format!("Changed {} files", changes.len())
        };
        let detail = changes
            .iter()
            .map(|change| format!("{}\n{}", change.path, change.diff))
            .collect::<Vec<_>>()
            .join("\n\n");
        Some(Self {
            title: title.into(),
            detail: append_progress(Some(detail), progress),
            status,
            additions,
            deletions,
        })
    }

    pub(super) fn status(&self) -> ToolStatus {
        self.status
    }
}

impl RenderOnce for FileChangeTool {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        ToolFrame::new(IconName::File, self.title, self.detail, self.status)
            .diff(self.additions, self.deletions)
    }
}

fn change_action(kind: &PatchChangeKind, status: ToolStatus) -> &'static str {
    let running = matches!(status, ToolStatus::Running);
    match kind {
        PatchChangeKind::Add if running => "Adding",
        PatchChangeKind::Add => "Added",
        PatchChangeKind::Delete if running => "Deleting",
        PatchChangeKind::Delete => "Deleted",
        PatchChangeKind::Update {
            move_path: None, ..
        } if running => "Editing",
        PatchChangeKind::Update {
            move_path: None, ..
        } => "Edited",
        PatchChangeKind::Update {
            move_path: Some(_), ..
        } if running => "Moving",
        PatchChangeKind::Update {
            move_path: Some(_), ..
        } => "Moved",
    }
}

fn diff_stats(diff: &str) -> (usize, usize) {
    let mut additions = 0;
    let mut deletions = 0;
    for line in diff.lines() {
        if line.starts_with("+++") || line.starts_with("---") {
            continue;
        }
        if line.starts_with('+') {
            additions += 1;
        } else if line.starts_with('-') {
            deletions += 1;
        }
    }
    (additions, deletions)
}
