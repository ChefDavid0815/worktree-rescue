# ADR 0001 — Core architecture: Rust core crate, Tauri shell, React UI

- Status: accepted
- Date: 2026-10-16

## Context

Worktree Rescue must read Git repositories, hash and store file contents, and
write recovered files with strong safety guarantees. The same logic has to be
testable against real repositories without a GUI, and must later run on
macOS and Linux.

## Decision

- **`crates/wr-core`** contains every piece of logic that touches a
  repository or the vault: Git invocation and parsing, worktree inventory and
  diagnosis, the snapshot engine, blob store, manifests, SQLite persistence,
  risk rules, retention, recovery planning and execution, forensics, and the
  demo generator. It has no Tauri dependency.
- **`src-tauri`** is a thin shell: typed IPC commands, a background monitor
  (file watcher, debounced scheduler, periodic inventory) and desktop
  integration (notifications, dialogs, single instance).
- **`src/`** is a React + TypeScript UI organised by feature
  (`overview`, `worktrees`, `snapshots`, `recovery`, `risks`, `settings`,
  `onboarding`) on a shared design system (`components/`, `styles/`).
- OS-specific behaviour (data directories, console-less child processes,
  volume reachability, reparse points) lives in `wr-core/src/platform.rs` and
  `paths.rs`.

## Consequences

- 49 Rust tests run against temporary Git repositories in seconds, without a
  window or WebView.
- The UI never touches the file system directly; it can only call typed
  commands whose inputs are validated in Rust.
