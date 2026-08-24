# gpui-component fork

This directory is a minimal source fork of
[`longbridge/gpui-component`](https://github.com/longbridge/gpui-component) at
commit `972a3ebfd01afca7da6d8b6f31c9a51288ea5565`.

Only the crates required by `codex-gui` are vendored. The local changes are
kept in the TextView implementation and are intended to be suitable for an
upstream pull request:

- keep the parsed Markdown block tree in an `Arc`, avoiding a recursive AST
  clone for each virtual-list frame;
- preserve the virtual-list scroll position across streaming Markdown appends
  by splicing only the reparsed block suffix instead of resetting `ListState`;
- promote top-level Markdown list items to independent virtual-list items while
  preserving ordered-list numbering, selection source, and append reparsing;
- derive selectable text hit geometry from GPUI's shaped line layouts instead
  of repeatedly scanning every character;
- accumulate per-frame selection geometry in a UI-thread-local shared buffer,
  avoiding an entity update for every visible inline.

The application continues to use the upstream `TextView` and `TextViewState`
APIs, including `TextViewState::push_str` for streaming appends.
