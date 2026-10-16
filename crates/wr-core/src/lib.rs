//! Worktree Rescue core library.
//!
//! This crate contains every piece of logic that touches user repositories or
//! the snapshot vault. It is deliberately independent from Tauri so it can be
//! tested against real temporary Git repositories and reused by other shells.
//!
//! Safety contract (enforced throughout the crate and covered by tests):
//! - Monitored repositories are only ever *read*. Git is invoked with
//!   `GIT_OPTIONAL_LOCKS=0` so even `git status` does not refresh the index.
//! - Writes are confined to the vault directory and to recovery targets the
//!   user explicitly selected.
//! - No destructive Git command (`reset --hard`, `clean`, `stash push`,
//!   `worktree remove/prune`, `checkout -- .`) is ever issued.

pub mod blob;
pub mod demo;
pub mod diagnostics;
pub mod error;
pub mod events;
pub mod filter;
pub mod forensics;
pub mod git;
pub mod manifest;
pub mod paths;
pub mod platform;
pub mod recovery;
pub mod retention;
pub mod risk;
pub mod settings;
pub mod snapshot;
pub mod store;
pub mod types;
pub mod vault;
pub mod worktree;

pub use error::{CoreError, Result};
pub use vault::Vault;

/// Current wall-clock time in milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
