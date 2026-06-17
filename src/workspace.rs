pub fn workspace_path() -> String {
    std::env::current_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "/Users/ken/Codes/codex-gui".into())
}
