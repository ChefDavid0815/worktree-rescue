# Contributing

Thanks for helping make AI-assisted development safer.

## Ground rules

1. **Data integrity over features.** Any change that touches a repository,
   the vault or a recovery target needs an integration test in
   `crates/wr-core/tests` using a real temporary Git repository.
2. **No destructive Git.** New Git invocations must pass
   `git::check_read_only`. If you think you need a mutating command, open an
   issue first.
3. **Honest UI.** The interface must never show inferred state as fact. Every
   new string needs an English and a Simplified Chinese entry
   (`src/lib/i18n/en.ts`, `zh.ts`); the type checker enforces parity.

## Development setup

Requirements: Rust (stable), Node.js 20+, Git 2.31+, and on Windows either
Visual Studio Build Tools (MSVC, recommended) or a MinGW-w64 toolchain.

```bash
npm install
npm run app:dev          # Tauri app with hot reload
cargo test -p wr-core    # core + integration tests
npm test                 # UI tests (Vitest)
npm run typecheck
```

Use `WR_DATA_DIR=<folder>` to run the app against an isolated data folder.

### Building with the GNU toolchain

GNU `ld` and `windres` cannot open paths containing non-ASCII characters. If
your checkout path contains them, point the target directory somewhere ASCII:

```bash
export CARGO_TARGET_DIR=/c/dev/wr-target
npx tauri build
```

## Pull requests

- Keep changes focused; describe the user-visible effect and the safety
  reasoning.
- Run `cargo fmt`, `cargo clippy -p wr-core --all-targets -- -D warnings`,
  `cargo test -p wr-core`, `npm run typecheck` and `npm test` before pushing.
- Architecture decisions go into `docs/adr/`.
