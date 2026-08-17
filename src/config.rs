use std::{env, fs, path::PathBuf};

pub fn codex_config_project_paths() -> Vec<String> {
    let Some(config_path) = codex_config_path() else {
        return Vec::new();
    };
    let Ok(config) = fs::read_to_string(config_path) else {
        return Vec::new();
    };

    parse_codex_config_project_paths(&config)
}

fn codex_config_path() -> Option<PathBuf> {
    env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex/config.toml"))
}

fn parse_codex_config_project_paths(config: &str) -> Vec<String> {
    let Ok(value) = config.parse::<toml::Value>() else {
        return Vec::new();
    };

    value
        .get("projects")
        .and_then(toml::Value::as_table)
        .map(|projects| projects.keys().cloned().collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::parse_codex_config_project_paths;

    #[test]
    fn reads_project_paths_from_codex_config() {
        let config = r#"
[projects."/Users/ken/Documents/Codex/2026-07-14/wo-xi"]
trust_level = "trusted"

[projects."/Users/ken/Codes/travel-companion"]
trust_level = "trusted"
"#;

        let mut paths = parse_codex_config_project_paths(config);
        paths.sort();

        assert_eq!(
            paths,
            vec![
                "/Users/ken/Codes/travel-companion".to_string(),
                "/Users/ken/Documents/Codex/2026-07-14/wo-xi".to_string(),
            ]
        );
    }

    #[test]
    fn ignores_missing_projects_table() {
        assert!(parse_codex_config_project_paths("model = \"gpt-5\"").is_empty());
    }
}
