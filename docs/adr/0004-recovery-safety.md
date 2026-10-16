# ADR 0004 — Recovery defaults to a separate directory; in-place restore is gated

- Status: accepted
- Date: 2026-10-16

## Decision

Recovery runs six recorded phases: source verification, target selection,
conflict detection, change preview, execution, post-recovery validation.

**Separate directory (default).** The target must be absolute, absent or
empty, and must not overlap any registered worktree, Git directory or vault.
Files are written via temp file + rename under `working-tree/` (and
`staged/` when staged content differs), every written file is re-hashed, and
`recovery-report.json` + `RECOVERY-README.md` are written next to them.
Unchanged files can optionally be materialised from the HEAD commit; each
such Git blob is verified against its object id and labelled
`existingGitObject`.

**In-place restore (opt-in, ≤ 200 files).**
1. A preview fingerprints every target (SHA-256, exact-case existence,
   regular file, no link ancestors). Previews are kept server-side and expire
   after 15 minutes; the UI cannot forge them.
2. Execution aborts *before writing anything* if any target changed since the
   preview.
3. A `preRestore` protection snapshot of the files about to be overwritten
   must complete as `verified`.
4. Each file is written to a temp file in the same directory, the target is
   re-hashed one last time, then atomically renamed over it and re-hashed.
5. Deletions are never replayed; symlinks are not recreated; the Git index is
   never modified.

## Consequences

Integration tests cover stale previews, corrupted sources, non-empty targets,
targets inside the worktree, and require the repository to be byte-identical
after any aborted restore.
