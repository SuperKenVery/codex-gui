# codex-gui

Opensource desktop GUI app for codex. No electron, built with `gpui`.

## Run

```sh
nix develop
cargo build --bin codex-code-mode-host
cargo run
```

The local code-mode host keeps Codex's upstream transport and session protocol,
but executes JavaScript with the patched QuickJS runtime in
`crates/codex-code-mode-runtime-quickjs`.
