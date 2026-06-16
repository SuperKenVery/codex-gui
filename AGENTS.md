# codex-gui

An opensource clone of the codex desktop app.

## Goals (features we want to implement)

- Build on Zed's GPUI, no electron
- Mimic the GUI layout and implement app-server bridging
  - Side bar with all projects and chats
  - Side chat (fork the current chat temporarily)
  - Fork chat (fork to new chat)
  - Normal chatting with streaming state update: commentary messages, tool calls, thinking animation etc.
- Use app server to communicate with codex
- 

## Non Goals (yet)

- Remote control (from phone or other codex desktop)
- Computer use, locked computer use
- Remote connection (controls other computers over ssh)
- Only support work locally, don't care about new worktree or cloud now
- Automations (recurrent tasks with timers)

## Dev environment

Manage environment and packaging with nix and cranelib.
