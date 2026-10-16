# ADR 0005 — Windows builds: MSVC in CI, GNU toolchain supported locally

- Status: accepted
- Date: 2026-10-16

## Context

The 1.0.0 release was produced on a machine without Visual Studio Build
Tools or administrator rights. Rust's `x86_64-pc-windows-gnu` toolchain with a
user-scope MinGW-w64 (WinLibs) works for Tauri 2, but GNU `ld`/`windres`
cannot open paths containing non-ASCII characters.

## Decision

- CI (GitHub Actions, `windows-latest`) builds with the default MSVC toolchain.
- Local GNU builds are supported: `src-tauri/build.rs` stages the window icon
  in `OUT_DIR`, and `CARGO_TARGET_DIR` must point to an ASCII path (see
  CONTRIBUTING.md).

## Consequences

Release artefacts attached to GitHub Releases state which toolchain produced
them. Code signing is not configured yet (see SECURITY.md).
