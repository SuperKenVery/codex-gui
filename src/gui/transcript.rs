use std::{
    collections::{HashMap, hash_map::DefaultHasher},
    hash::{Hash as _, Hasher as _},
    sync::{Arc, RwLock},
    time::Duration,
};

use gpui::{App, Entity, EntityId, IntoElement, SharedString, WeakEntity, Window, div};
use gpui_component::text::{
    MarkdownExtensions, MarkdownNode, MarkdownParseContext, MarkdownPlugin, markdown_ast,
};

use super::{HistoryKey, MessageState, chat_history::ChatHistory, widgets::ToolCallView};

const BLOCK_TAG: &str = "CodexTranscriptBlock";

pub(super) type TranscriptBlockStore = Arc<RwLock<HashMap<String, TranscriptBlockTarget>>>;

#[derive(Clone)]
pub(super) enum TranscriptBlockTarget {
    User {
        key: HistoryKey,
        body: SharedString,
    },
    AssistantHeader {
        key: HistoryKey,
        label: &'static str,
    },
    Tools {
        key: HistoryKey,
        message: Entity<MessageState>,
        tools: Arc<[ToolCallView]>,
        expanded: bool,
        collapse: bool,
        active_tail: bool,
    },
    WorkedSummary {
        turn_id: EntityId,
        duration: Duration,
        expanded: bool,
    },
}

impl TranscriptBlockTarget {
    fn id(&self) -> String {
        let mut hasher = DefaultHasher::new();
        match self {
            Self::User { key, .. } => {
                "user".hash(&mut hasher);
                key.hash(&mut hasher);
            }
            Self::AssistantHeader { key, .. } => {
                "assistant-header".hash(&mut hasher);
                key.hash(&mut hasher);
            }
            Self::Tools { key, .. } => {
                "tools".hash(&mut hasher);
                key.hash(&mut hasher);
            }
            Self::WorkedSummary { turn_id, .. } => {
                "worked-summary".hash(&mut hasher);
                turn_id.hash(&mut hasher);
            }
        }
        format!("block-{:016x}", hasher.finish())
    }
}

pub(super) struct TranscriptDocument {
    pub source: String,
    pub blocks: HashMap<String, TranscriptBlockTarget>,
}

impl TranscriptDocument {
    pub fn new() -> Self {
        Self {
            source: String::new(),
            blocks: HashMap::new(),
        }
    }

    pub fn push_markdown(&mut self, markdown: &str) {
        if markdown.is_empty() {
            return;
        }
        self.push_separator();
        self.source.push_str(markdown);
    }

    pub fn push_block(&mut self, target: TranscriptBlockTarget) {
        let id = target.id();
        self.push_separator();
        self.source
            .push_str(&format!(r#"<{BLOCK_TAG} id="{id}" />"#));
        self.blocks.insert(id, target);
    }

    fn push_separator(&mut self) {
        if !self.source.is_empty() {
            self.source.push_str("\n\n");
        }
    }
}

#[derive(Clone)]
pub(super) struct TranscriptPlugin {
    history: WeakEntity<ChatHistory>,
    blocks: TranscriptBlockStore,
}

impl TranscriptPlugin {
    pub fn new(history: WeakEntity<ChatHistory>, blocks: TranscriptBlockStore) -> Self {
        Self { history, blocks }
    }

    pub fn extensions(self) -> MarkdownExtensions {
        MarkdownExtensions::default().plugin(self)
    }
}

#[derive(Clone)]
struct TranscriptNode {
    id: String,
}

impl MarkdownPlugin for TranscriptPlugin {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "codex-transcript-block"
    }

    fn parse(
        &self,
        node: &markdown_ast::Node,
        cx: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        let markdown_ast::Node::Html(raw) = node else {
            return None;
        };
        if html_tag_name(&raw.value) != Some(BLOCK_TAG) {
            return None;
        }
        let id = html_attr(&raw.value, "id")?;
        if !self
            .blocks
            .read()
            .ok()
            .is_some_and(|blocks| blocks.contains_key(&id))
        {
            return None;
        }

        Some(
            MarkdownNode::new("codex-transcript-block", TranscriptNode { id })
                .markdown(cx.node_source(node).unwrap_or(raw.value.as_str())),
        )
    }

    fn render(&self, node: &MarkdownNode, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let target = node
            .data::<TranscriptNode>()
            .and_then(|node| self.blocks.read().ok()?.get(&node.id).cloned());

        target
            .map(|target| {
                super::chat_history::render_transcript_block(&self.history, target, window, cx)
            })
            .unwrap_or_else(|| div().into_any_element())
    }
}

fn html_tag_name(value: &str) -> Option<&str> {
    value
        .trim()
        .strip_prefix('<')?
        .split([' ', '/', '>'])
        .next()
}

fn html_attr(value: &str, name: &str) -> Option<String> {
    let pattern = format!(r#"{name}=""#);
    let start = value.find(&pattern)? + pattern.len();
    let end = value[start..].find('"')?;
    Some(value[start..start + end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_markdown_is_a_strict_source_append() {
        let mut before = TranscriptDocument::new();
        before.push_block(TranscriptBlockTarget::User {
            key: HistoryKey::Item("user-1".into()),
            body: "hello".into(),
        });
        before.push_markdown("A partial reply");

        let mut after = TranscriptDocument::new();
        after.push_block(TranscriptBlockTarget::User {
            key: HistoryKey::Item("user-1".into()),
            body: "hello".into(),
        });
        after.push_markdown("A partial reply with another chunk");

        assert_eq!(
            after.source.strip_prefix(&before.source),
            Some(" with another chunk")
        );
    }

    #[test]
    fn dynamic_block_state_does_not_change_the_source_marker() {
        let key = HistoryKey::Item("assistant-1".into());

        let mut before = TranscriptDocument::new();
        before.push_block(TranscriptBlockTarget::AssistantHeader {
            key: key.clone(),
            label: "Codex is working",
        });

        let mut after = TranscriptDocument::new();
        after.push_block(TranscriptBlockTarget::AssistantHeader {
            key,
            label: "Codex",
        });

        assert_eq!(after.source, before.source);
    }

    #[test]
    fn parses_only_the_transcript_tag_and_quoted_id() {
        assert_eq!(
            html_tag_name(r#" <CodexTranscriptBlock id="block-1" /> "#),
            Some(BLOCK_TAG)
        );
        assert_eq!(
            html_attr(r#"<CodexTranscriptBlock id="block-1" />"#, "id"),
            Some("block-1".into())
        );
        assert_eq!(html_tag_name("<OtherBlock />"), Some("OtherBlock"));
        assert_eq!(html_attr("<CodexTranscriptBlock />", "id"), None);
    }
}
