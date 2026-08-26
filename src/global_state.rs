use anyhow::{Context as _, Result, anyhow};
use codex_core::config::find_codex_home;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    collections::HashSet,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

const STATE_FILE_NAME: &str = ".codex-global-state.json";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct CodexGlobalState {
    #[serde(
        default,
        rename = "projectless-thread-ids",
        skip_serializing_if = "Vec::is_empty"
    )]
    projectless_thread_ids: Vec<String>,
    #[serde(
        default,
        rename = "electron-persisted-atom-state",
        skip_serializing_if = "Option::is_none"
    )]
    persisted_atom_state: Option<PersistedAtomState>,
    #[serde(flatten)]
    other: Map<String, Value>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct PersistedAtomState {
    #[serde(
        default,
        rename = "chatgpt-last-selected-model-v1",
        skip_serializing_if = "Option::is_none"
    )]
    last_selected_model: Option<LastSelectedModel>,
    #[serde(flatten)]
    other: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) struct LastSelectedModel {
    pub(crate) slug: String,
    #[serde(
        default,
        rename = "thinkingEffort",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) thinking_effort: Option<String>,
    #[serde(default, rename = "versionId", skip_serializing_if = "Option::is_none")]
    version_id: Option<String>,
    #[serde(flatten)]
    other: Map<String, Value>,
}

impl CodexGlobalState {
    pub(crate) fn load() -> Result<Self> {
        let codex_home = find_codex_home().context("failed to locate CODEX_HOME")?;
        Self::load_from_path(&codex_home.join(STATE_FILE_NAME))
    }

    pub(crate) fn projectless_thread_ids(&self) -> HashSet<String> {
        self.projectless_thread_ids.iter().cloned().collect()
    }

    pub(crate) fn last_selected_model(&self) -> Option<LastSelectedModel> {
        self.persisted_atom_state
            .as_ref()?
            .last_selected_model
            .clone()
    }

    pub(crate) fn update_last_selected_model(model: &str, effort: &str) -> Result<()> {
        Self::update(|state| {
            let atoms = state
                .persisted_atom_state
                .get_or_insert_with(PersistedAtomState::default);
            let selection = atoms
                .last_selected_model
                .get_or_insert_with(|| LastSelectedModel {
                    slug: String::new(),
                    thinking_effort: None,
                    version_id: Some("latest".into()),
                    other: Map::new(),
                });
            selection.slug = desktop_model_slug(model);
            selection.thinking_effort = Some(desktop_thinking_effort(effort).into());
            selection.version_id.get_or_insert_with(|| "latest".into());
        })
    }

    pub(crate) fn add_projectless_thread(thread_id: &str) -> Result<()> {
        Self::update(|state| {
            if !state
                .projectless_thread_ids
                .iter()
                .any(|existing| existing == thread_id)
            {
                state.projectless_thread_ids.push(thread_id.to_owned());
            }
        })
    }

    fn update(mutator: impl FnOnce(&mut Self)) -> Result<()> {
        let codex_home = find_codex_home().context("failed to locate CODEX_HOME")?;
        let path = codex_home.join(STATE_FILE_NAME);
        let mut state = Self::load_from_path(&path)?;
        mutator(&mut state);
        state.save_to_path(&path)
    }

    fn load_from_path(path: &Path) -> Result<Self> {
        match fs::read_to_string(path) {
            Ok(contents) => match serde_json::from_str(&contents) {
                Ok(state) => Ok(state),
                Err(primary_error) => {
                    let backup_path = backup_path(path);
                    let backup = fs::read_to_string(&backup_path).with_context(|| {
                        format!(
                            "failed to parse {} and read backup {}: {primary_error}",
                            path.display(),
                            backup_path.display()
                        )
                    })?;
                    serde_json::from_str(&backup).with_context(|| {
                        format!(
                            "failed to parse {} and backup {}",
                            path.display(),
                            backup_path.display()
                        )
                    })
                }
            },
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let backup_path = backup_path(path);
                match fs::read_to_string(&backup_path) {
                    Ok(contents) => serde_json::from_str(&contents)
                        .with_context(|| format!("failed to parse {}", backup_path.display())),
                    Err(backup_error) if backup_error.kind() == ErrorKind::NotFound => {
                        Ok(Self::default())
                    }
                    Err(backup_error) => Err(backup_error)
                        .with_context(|| format!("failed to read {}", backup_path.display())),
                }
            }
            Err(error) => Err(error).with_context(|| format!("failed to read {}", path.display())),
        }
    }

    fn save_to_path(&self, path: &Path) -> Result<()> {
        let contents = format!("{}\n", serde_json::to_string(self)?);
        atomic_write(path, contents.as_bytes())?;
        atomic_write(&backup_path(path), contents.as_bytes())?;
        Ok(())
    }
}

fn desktop_model_slug(model: &str) -> String {
    let Some(model_name) = model.strip_prefix("gpt-") else {
        return model.to_owned();
    };
    let version = model_name.strip_suffix("-sol").unwrap_or(model_name);
    if version
        .chars()
        .all(|character| character.is_ascii_digit() || character == '.')
    {
        format!("gpt-{}-thinking", version.replace('.', "-"))
    } else {
        model.to_owned()
    }
}

fn desktop_thinking_effort(effort: &str) -> &str {
    match effort {
        "medium" => "standard",
        "high" => "extended",
        effort => effort,
    }
}

fn backup_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.bak", path.display()))
}

fn atomic_write(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent directory", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| anyhow!("{} has no file name", path.display()))?
        .to_string_lossy();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let temporary_path = parent.join(format!(".{file_name}.tmp-{timestamp}-{}", Uuid::new_v4()));

    let result = fs::write(&temporary_path, contents)
        .with_context(|| format!("failed to write {}", temporary_path.display()))
        .and_then(|_| {
            fs::rename(&temporary_path, path).with_context(|| {
                format!(
                    "failed to replace {} with {}",
                    path.display(),
                    temporary_path.display()
                )
            })
        });
    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_unknown_top_level_and_nested_fields() {
        let mut state: CodexGlobalState = serde_json::from_str(
            r#"{
                "unknown-top-level": {"keep": true},
                "projectless-thread-ids": ["abc"],
                "electron-persisted-atom-state": {
                    "unknown-atom": [1, 2, 3],
                    "chatgpt-last-selected-model-v1": {
                        "slug": "gpt-5-6-thinking",
                        "thinkingEffort": "extended",
                        "versionId": "latest",
                        "unknown-selection": 42
                    }
                }
            }"#,
        )
        .unwrap();

        state.projectless_thread_ids.push("def".into());
        let value = serde_json::to_value(state).unwrap();
        assert_eq!(value["unknown-top-level"]["keep"], true);
        assert_eq!(
            value["electron-persisted-atom-state"]["unknown-atom"],
            serde_json::json!([1, 2, 3])
        );
        assert_eq!(
            value["electron-persisted-atom-state"]["chatgpt-last-selected-model-v1"]["unknown-selection"],
            42
        );
        assert_eq!(value["projectless-thread-ids"][1], "def");
    }

    #[test]
    fn parses_last_selected_model() {
        let state: CodexGlobalState = serde_json::from_str(
            r#"{
                "electron-persisted-atom-state": {
                    "chatgpt-last-selected-model-v1": {
                        "slug": "gpt-5-6-thinking",
                        "thinkingEffort": "extended",
                        "versionId": "latest"
                    }
                }
            }"#,
        )
        .unwrap();
        let selection = state.last_selected_model().unwrap();
        assert_eq!(selection.slug, "gpt-5-6-thinking");
        assert_eq!(selection.thinking_effort.as_deref(), Some("extended"));
    }

    #[test]
    fn maps_codex_model_settings_to_desktop_values() {
        assert_eq!(desktop_model_slug("gpt-5.6-sol"), "gpt-5-6-thinking");
        assert_eq!(desktop_model_slug("gpt-5.5"), "gpt-5-5-thinking");
        assert_eq!(desktop_model_slug("custom-model"), "custom-model");
        assert_eq!(desktop_thinking_effort("medium"), "standard");
        assert_eq!(desktop_thinking_effort("high"), "extended");
        assert_eq!(desktop_thinking_effort("xhigh"), "xhigh");
    }

    #[test]
    fn atomic_save_creates_matching_primary_and_backup() {
        let directory = std::env::temp_dir().join(format!("codex-gui-state-{}", Uuid::new_v4()));
        let path = directory.join(STATE_FILE_NAME);
        let state = CodexGlobalState {
            projectless_thread_ids: vec!["thread-1".into()],
            ..Default::default()
        };

        state.save_to_path(&path).unwrap();

        assert_eq!(
            fs::read(&path).unwrap(),
            fs::read(backup_path(&path)).unwrap()
        );
        assert_eq!(
            CodexGlobalState::load_from_path(&path)
                .unwrap()
                .projectless_thread_ids,
            vec!["thread-1"]
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
