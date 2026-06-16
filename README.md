# codex-gui

An experimental GPUI desktop shell for a Codex-like local app.

## Run

```sh
nix develop -c cargo run
```

## Build

```sh
nix build
```

The Nix package is built with `ipetkov/crane` and the latest stable Rust from
`rust-overlay`. GPUI is resolved from `zed-industries/zed` on GitHub so builds
do not depend on a local checkout.

The first implementation is a native GPUI scaffold with:

- project and chat sidebar
- primary chat transcript
- temporary side chat panel
- fork-chat action
- mocked streaming/tool-call state shaped for a future codex app-server transport
