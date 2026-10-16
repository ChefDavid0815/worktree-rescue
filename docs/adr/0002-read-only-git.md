# ADR 0002 — Git is invoked read-only through an allow-list

- Status: accepted
- Date: 2026-10-16

## Context

A tool that protects uncommitted work must never be the cause of losing it.
`git status` itself may rewrite `.git/index` to refresh stat information, and
convenient shortcuts (`stash push`, `reset --hard`, `worktree prune`) destroy
or move data.

## Decision

- Use the system Git executable through `std::process::Command` with an
  argument vector (no shell, no string concatenation).
- Every product invocation passes `check_read_only`, an allow-list of
  sub-commands with per-command argument checks (`worktree list` only,
  `stash list|show` only, `config --get*` only, no `fsck --lost-found`,
  no `diff --output`, no reflog expiry, no `symbolic-ref` writes).
- Set `GIT_OPTIONAL_LOCKS=0` so status never refreshes the index, plus
  `core.fsmonitor=false`, `LC_ALL=C`, `GIT_TERMINAL_PROMPT=0`, and remove
  inherited `GIT_DIR`/`GIT_WORK_TREE`/`GIT_INDEX_FILE`.
- Every call has a timeout and a cancellation flag; on expiry the process is
  killed.
- Mutating Git commands exist only for building the sandbox demo and test
  fixtures (`run_fixture_write`), which refuses any working directory outside
  the sandbox root.

## Consequences

- `FORBIDDEN_EXAMPLES` is asserted in unit tests.
- An integration test fingerprints the working tree, index, `HEAD`, refs,
  reflogs and stash list before and after inventory, snapshots, verification,
  forensic scans, previews, retention and separate-directory recovery, and
  requires them to be byte-identical.
- `git worktree repair` and `prune` are shown as *guidance* only.
