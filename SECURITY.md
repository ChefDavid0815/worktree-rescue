# Security Policy

## Supported versions

| Version | Supported |
| ------- | --------- |
| 1.0.x   | ✅        |

## Reporting a vulnerability

Please report security issues privately through
[GitHub Security Advisories](https://github.com/ChefDavid0815/worktree-rescue/security/advisories/new)
rather than public issues. Include the version, steps to reproduce and the
impact you observed. You can expect an initial response within a week.

## Security model

- **Local only.** Worktree Rescue makes no network requests, has no telemetry
  and never uploads source code or backups.
- **Read-only repositories.** Git is executed with an allow-list of read-only
  sub-commands (see `crates/wr-core/src/git.rs`) and `GIT_OPTIONAL_LOCKS=0`.
  Destructive commands (`reset --hard`, `clean`, `stash push`,
  `worktree remove/prune/repair`, …) are rejected by a unit-tested guard.
- **Bounded writes.** The application writes only to its data folder
  (`%LOCALAPPDATA%\WorktreeRescue`) and to recovery destinations you choose.
  In-place restore requires a fresh preview, a verified protection snapshot
  and explicit confirmation.
- **Untrusted paths.** Paths from Git output and manifests are validated
  before use; symbolic links and junctions are recorded, never followed.
- **Sensitive files** (`.env`, keys) are excluded from capture by default.
  When included, they are stored unencrypted in the local vault, protected by
  your Windows account permissions. Full-disk encryption (BitLocker) is
  recommended.

## Code signing

Release 1.0.0 installers are **not code-signed**; Windows SmartScreen may show
a warning. The release workflow (`.github/workflows/release.yml`) marks where a
signing step (Azure Trusted Signing or `signtool` with a certificate held in
repository secrets) will be added. Verify downloads against the SHA-256
checksum published with each release.
