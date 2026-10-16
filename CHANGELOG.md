# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [1.0.0] — 2026-10-16

First public release.

### Added
- **Worktree Intelligence** — inventory of the main and every linked worktree
  from `git worktree list --porcelain -z`, with evidence-based diagnosis:
  healthy, missing, possibly moved (pointer found in a sibling directory),
  prunable candidate, drive unavailable, inaccessible, unknown. Locks,
  detached HEAD and unborn branches are shown as such.
- **Independent Snapshot Engine** — captures staged content, working-tree
  bytes, untracked files, deletions and renames into a content-addressed
  SHA-256 vault with atomic JSON manifests and a SQLite index. Snapshots are
  `verified` only after re-hashing and a committed transaction; `partial`,
  `failed` and `interrupted` states are reported honestly.
- **Automatic protection** (opt-in per repository) — file-system watcher with
  a 20 s quiet period, 60 s minimum interval and 10 min checkpoints during
  continuous editing; unchanged states are confirmed instead of duplicated.
- **Capture policy** — per-file size limit, per-repository budget, default
  exclusion of dependency/build caches, sensitive-file patterns, explicit
  include rules for Git-ignored paths; every skipped file is listed with its
  reason.
- **Recorder & Timeline** — per-worktree time strip of snapshots, Git events,
  exposure windows and unreachability; filterable, virtualized timeline with
  file-path search and grouping by worktree.
- **Diff viewer** (Monaco) — HEAD → snapshot, snapshot → current file,
  staged → working tree, with per-file version history across snapshots and
  binary/LFS metadata.
- **Safe Recovery Workspace** — six recorded phases; recover to a separate
  folder by default (optionally including staged versions and unchanged
  files from the HEAD commit); gated in-place restore with server-side
  previews, verified protection snapshots, atomic replacement and hash
  validation. The Git index is never modified.
- **Git history search** — read-only reflog, stash and `fsck --unreachable`
  discovery with provenance labels and verified export.
- **Risk Observatory** — local rules for unreachable worktrees with
  uncaptured work, stale protection, failing automatic snapshots, low disk
  space, budget pressure, unprotected files, mass deletions and integrity
  failures, each with severity, confidence and raw evidence.
- **Storage management** — retention policy (keep-all window, hourly, daily,
  per-worktree cap, pins), reference-checked garbage collection and orphan
  sweep.
- **Sandbox demo** — generated repository with three agent worktrees, one
  deleted and one moved, stored in a separate vault.
- **Settings** — protection timing, capture scope, budgets, retention,
  recovery folder, Git path, notifications, theme (system/dark/light) and
  language (English / 简体中文).
- **Diagnostics export** without file contents.

### Security
- Read-only Git allow-list, `GIT_OPTIONAL_LOCKS=0`, argument-vector process
  execution with timeouts and cancellation.
- Path validation against traversal, absolute paths, Windows device names,
  alternate data streams and case collisions; links and junctions are never
  followed.
