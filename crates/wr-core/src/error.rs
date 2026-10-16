use serde::Serialize;
use std::path::PathBuf;

/// Errors produced by the core library.
///
/// Every variant carries a stable machine-readable `code()` so the UI can
/// translate it without parsing English text.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("Git executable was not found. Install Git for Windows or set its path in Settings.")]
    GitNotFound,
    #[error("Git command `{command}` failed (exit {code:?}): {stderr}")]
    GitFailed {
        command: String,
        code: Option<i32>,
        stderr: String,
    },
    #[error("Git command `{command}` timed out after {seconds}s")]
    GitTimeout { command: String, seconds: u64 },
    #[error("Operation was cancelled")]
    Cancelled,
    #[error("Not a Git repository: {0}")]
    NotARepository(PathBuf),
    #[error("Bare repositories are not supported: {0}")]
    BareRepository(PathBuf),
    #[error("Unsafe path rejected: {0}")]
    UnsafePath(String),
    #[error("Project not found: {0}")]
    ProjectNotFound(String),
    #[error("Snapshot not found: {0}")]
    SnapshotNotFound(String),
    #[error("Project already registered: {0}")]
    AlreadyRegistered(String),
    #[error("Integrity check failed for {what}: expected {expected}, found {actual}")]
    Integrity {
        what: String,
        expected: String,
        actual: String,
    },
    #[error("Recovery target is not acceptable: {0}")]
    InvalidTarget(String),
    #[error("Recovery aborted: {0}")]
    RecoveryAborted(String),
    #[error("Not enough disk space: {needed} bytes needed, {available} available")]
    InsufficientSpace { needed: u64, available: u64 },
    #[error("Storage budget exceeded: {used} of {budget} bytes")]
    BudgetExceeded { used: u64, budget: u64 },
    #[error("Invalid manifest: {0}")]
    InvalidManifest(String),
    #[error("Invalid setting: {0}")]
    InvalidSetting(String),
    #[error("A task is already running for this worktree")]
    Busy,
    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("Serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Other(String),
}

impl CoreError {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        CoreError::Io {
            path: path.into(),
            source,
        }
    }

    /// Stable identifier used by the UI for localisation.
    pub fn code(&self) -> &'static str {
        match self {
            CoreError::GitNotFound => "git_not_found",
            CoreError::GitFailed { .. } => "git_failed",
            CoreError::GitTimeout { .. } => "git_timeout",
            CoreError::Cancelled => "cancelled",
            CoreError::NotARepository(_) => "not_a_repository",
            CoreError::BareRepository(_) => "bare_repository",
            CoreError::UnsafePath(_) => "unsafe_path",
            CoreError::ProjectNotFound(_) => "project_not_found",
            CoreError::SnapshotNotFound(_) => "snapshot_not_found",
            CoreError::AlreadyRegistered(_) => "already_registered",
            CoreError::Integrity { .. } => "integrity",
            CoreError::InvalidTarget(_) => "invalid_target",
            CoreError::RecoveryAborted(_) => "recovery_aborted",
            CoreError::InsufficientSpace { .. } => "insufficient_space",
            CoreError::BudgetExceeded { .. } => "budget_exceeded",
            CoreError::InvalidManifest(_) => "invalid_manifest",
            CoreError::InvalidSetting(_) => "invalid_setting",
            CoreError::Busy => "busy",
            CoreError::Io { .. } => "io",
            CoreError::Db(_) => "db",
            CoreError::Json(_) => "json",
            CoreError::Other(_) => "other",
        }
    }
}

/// Serializable error shape returned across the IPC boundary.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorPayload {
    pub code: String,
    pub message: String,
}

impl From<&CoreError> for ErrorPayload {
    fn from(e: &CoreError) -> Self {
        ErrorPayload {
            code: e.code().to_string(),
            message: e.to_string(),
        }
    }
}

pub type Result<T> = std::result::Result<T, CoreError>;
