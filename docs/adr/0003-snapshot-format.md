# ADR 0003 — Snapshot storage: content-addressed blobs + JSON manifests + SQLite index

- Status: accepted
- Date: 2026-10-16

## Context

Snapshots must be independent of Git (commits, stash, index), deduplicated,
verifiable without trusting the database, and robust against crashes, disk
exhaustion and files changing while they are read.

## Decision

- **Blobs** are stored at `blobs/sha256/<aa>/<bb>/<sha256>`. They are written
  to `blobs/tmp`, flushed with `fsync`, hashed while streaming and renamed
  into place. The file name *is* the hash, so integrity can always be
  re-checked.
- **Manifests** (`manifests/<id[0..2]>/<id>.json`, format version 1) list every
  entry: repository-relative path, original path for renames, Git status
  letters, HEAD object id, staged object id + vault blob, working-tree state
  (`present`, `absent`, `skipped`, `failed`), kind (`file`, `symlink`,
  `submodule`), size, executable bit, skip reason, LFS-pointer and binary
  flags, plus the Git context (branch, HEAD, detached/unborn, sparse).
  Manifests are written atomically and their SHA-256 is stored in SQLite.
- **SQLite** (WAL, `synchronous=FULL`, versioned migrations) indexes projects,
  worktrees, snapshots, entries, blobs, events, risks and recoveries.
- **Commit order:** blobs → re-hash of newly written blobs → manifest written
  and read back → one database transaction. Only then is a snapshot
  `verified` (or `partial` if included files could not be captured).
  Snapshots found `inProgress` at start-up become `interrupted`.
- **Change detection:** each file is fingerprinted (size + mtime) before and
  after reading; a mismatch retries up to three times and is recorded as
  `unstable` rather than silently accepted.
- **Deletion:** retention and manual deletion remove database rows first,
  then manifests, then blobs no remaining entry references (checked again
  immediately before each delete), all while captures are excluded by a
  read/write gate.
- Symlinks and Windows reparse points are recorded by their link text and
  never followed.

## Consequences

- A manifest plus the blob store is sufficient to rebuild every captured file.
- Untracked files are captured by policy; Git-ignored files only via explicit
  include rules; sensitive files only when opted in.
- Submodule contents, sparse-checkout gaps and LFS objects are reported as
  partial support instead of being guessed.
