//! Data transfer objects shared with the UI. Every struct serialises to
//! camelCase JSON; the TypeScript mirror lives in `src/lib/types.ts`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Monitoring {
    Active,
    Paused,
}

impl Monitoring {
    pub fn as_str(&self) -> &'static str {
        match self {
            Monitoring::Active => "active",
            Monitoring::Paused => "paused",
        }
    }
    pub fn parse(s: &str) -> Self {
        if s == "paused" {
            Monitoring::Paused
        } else {
            Monitoring::Active
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    /// Main worktree path (display form).
    pub root_path: String,
    pub common_dir: String,
    pub is_demo: bool,
    pub added_at: i64,
    pub auto_protect: bool,
    pub monitoring: Monitoring,
    pub last_snapshot_at: Option<i64>,
    pub last_verified_snapshot_at: Option<i64>,
    pub snapshot_count: i64,
    pub stored_bytes: i64,
    pub open_risks: RiskCounts,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskCounts {
    pub info: i64,
    pub attention: i64,
    pub critical: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeDiagnosis {
    /// Path exists and its `.git` pointer resolves to this repository.
    Healthy,
    /// Volume is mounted but the directory is gone; no relocation evidence.
    Missing,
    /// Directory is gone but a sibling directory points at the same
    /// worktree metadata.
    PossiblyMoved,
    /// Git itself reports the worktree as prunable and nothing suggests the
    /// volume is merely unavailable.
    PrunableCandidate,
    /// The drive or network share is not reachable; data may be intact.
    VolumeUnavailable,
    /// The path exists but cannot be read (permissions, locks).
    Inaccessible,
    /// Evidence is inconclusive (e.g. `.git` pointer mismatch).
    Unknown,
}

impl WorktreeDiagnosis {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorktreeDiagnosis::Healthy => "healthy",
            WorktreeDiagnosis::Missing => "missing",
            WorktreeDiagnosis::PossiblyMoved => "possiblyMoved",
            WorktreeDiagnosis::PrunableCandidate => "prunableCandidate",
            WorktreeDiagnosis::VolumeUnavailable => "volumeUnavailable",
            WorktreeDiagnosis::Inaccessible => "inaccessible",
            WorktreeDiagnosis::Unknown => "unknown",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "healthy" => Self::Healthy,
            "missing" => Self::Missing,
            "possiblyMoved" => Self::PossiblyMoved,
            "prunableCandidate" => Self::PrunableCandidate,
            "volumeUnavailable" => Self::VolumeUnavailable,
            "inaccessible" => Self::Inaccessible,
            _ => Self::Unknown,
        }
    }
    pub fn is_accessible(&self) -> bool {
        matches!(self, WorktreeDiagnosis::Healthy)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StatusCounts {
    pub staged: i64,
    pub unstaged: i64,
    pub untracked: i64,
    pub conflicted: i64,
    pub deleted: i64,
    pub tracked_paths: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    pub id: String,
    pub project_id: String,
    pub path: String,
    pub is_main: bool,
    pub branch: Option<String>,
    pub head_oid: Option<String>,
    pub detached: bool,
    pub locked: bool,
    pub lock_reason: Option<String>,
    pub git_prunable: bool,
    pub prunable_reason: Option<String>,
    pub diagnosis: WorktreeDiagnosis,
    /// Evidence strings (codes) supporting the diagnosis.
    pub evidence: Vec<String>,
    pub possible_new_path: Option<String>,
    pub dirty: Option<bool>,
    pub counts: Option<StatusCounts>,
    pub first_seen_at: i64,
    pub last_seen_at: Option<i64>,
    pub last_checked_at: i64,
    /// Last time the uncommitted state was observed to *change*
    /// (a new dirty fingerprint), not merely observed dirty.
    pub last_dirty_at: Option<i64>,
    /// Fingerprint of the dirty state (paths, status letters, sizes, mtimes).
    pub dirty_fingerprint: Option<String>,
    /// Last time an automatic capture confirmed nothing changed since the
    /// latest snapshot.
    pub last_confirmed_at: Option<i64>,
    /// max(last snapshot, last confirmation): the state is protected as of
    /// this moment.
    pub last_protected_at: Option<i64>,
    pub last_snapshot_id: Option<String>,
    pub last_snapshot_at: Option<i64>,
    pub last_verified_snapshot_at: Option<i64>,
    pub gone_from_git: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SnapshotTrigger {
    Manual,
    Auto,
    Checkpoint,
    PreRestore,
}

impl SnapshotTrigger {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Auto => "auto",
            Self::Checkpoint => "checkpoint",
            Self::PreRestore => "preRestore",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "auto" => Self::Auto,
            "checkpoint" => Self::Checkpoint,
            "preRestore" => Self::PreRestore,
            _ => Self::Manual,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SnapshotStatus {
    /// Capture is still running (or the app died while it was running).
    InProgress,
    /// Every included entry captured and verified.
    Verified,
    /// Manifest committed and verified, but some included entries could not
    /// be captured (locked, changed during read, permission denied…).
    Partial,
    /// Nothing usable was committed.
    Failed,
    /// Found `InProgress` after a restart: the process died mid-capture.
    Interrupted,
}

impl SnapshotStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InProgress => "inProgress",
            Self::Verified => "verified",
            Self::Partial => "partial",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "verified" => Self::Verified,
            "partial" => Self::Partial,
            "failed" => Self::Failed,
            "interrupted" => Self::Interrupted,
            _ => Self::InProgress,
        }
    }
    pub fn is_recoverable(&self) -> bool {
        matches!(self, Self::Verified | Self::Partial)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotSummary {
    pub id: String,
    pub project_id: String,
    pub worktree_path: String,
    pub created_at: i64,
    pub finished_at: Option<i64>,
    pub trigger: SnapshotTrigger,
    pub status: SnapshotStatus,
    pub branch: Option<String>,
    pub head_oid: Option<String>,
    pub detached: bool,
    pub entry_count: i64,
    pub captured_count: i64,
    pub skipped_count: i64,
    pub failed_count: i64,
    pub staged_count: i64,
    pub unstaged_count: i64,
    pub untracked_count: i64,
    pub deleted_count: i64,
    pub protected_bytes: i64,
    pub new_bytes: i64,
    pub manifest_sha256: Option<String>,
    pub pinned: bool,
    pub note: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub verified_at: Option<i64>,
}

/// Per-file row used by the snapshot browser (mirrors the manifest entry).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotFile {
    pub path: String,
    pub orig_path: Option<String>,
    pub category: String,
    pub x: String,
    pub y: String,
    pub kind: String,
    pub wt_state: String,
    pub wt_blob: Option<String>,
    pub wt_size: Option<i64>,
    pub index_blob: Option<String>,
    pub index_oid: Option<String>,
    pub head_oid: Option<String>,
    pub skip_reason: Option<String>,
    pub link_target: Option<String>,
    pub lfs_pointer: bool,
    pub binary: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    Info,
    Attention,
    Critical,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Attention => "attention",
            Self::Critical => "critical",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "critical" => Self::Critical,
            "attention" => Self::Attention,
            _ => Self::Info,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

impl Confidence {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "high" => Self::High,
            "medium" => Self::Medium,
            _ => Self::Low,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Risk {
    pub id: String,
    pub project_id: String,
    pub worktree_path: Option<String>,
    /// Stable rule identifier; the UI owns titles and explanations.
    pub rule: String,
    pub severity: Severity,
    pub confidence: Confidence,
    pub detected_at: i64,
    pub last_seen_at: i64,
    pub resolved_at: Option<i64>,
    pub acknowledged: bool,
    /// Raw detection evidence (also used as translation parameters).
    pub evidence: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineEvent {
    pub id: i64,
    pub project_id: String,
    pub worktree_path: Option<String>,
    pub at: i64,
    pub kind: String,
    pub snapshot_id: Option<String>,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventQuery {
    pub project_id: String,
    pub worktree_path: Option<String>,
    pub kinds: Option<Vec<String>>,
    pub since: Option<i64>,
    pub until: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotQuery {
    pub project_id: String,
    pub worktree_path: Option<String>,
    pub since: Option<i64>,
    pub until: Option<i64>,
    /// Only snapshots containing a file whose path contains this text.
    pub path_contains: Option<String>,
    pub limit: Option<i64>,
}

/// Version history of a single file across snapshots.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileVersion {
    pub snapshot_id: String,
    pub created_at: i64,
    pub worktree_path: String,
    pub branch: Option<String>,
    pub status: SnapshotStatus,
    pub file: SnapshotFile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageStats {
    pub data_dir: String,
    pub blob_count: i64,
    pub blob_bytes: i64,
    pub manifest_count: i64,
    pub snapshot_count: i64,
    pub available_bytes: Option<u64>,
    pub per_project: Vec<ProjectStorage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStorage {
    pub project_id: String,
    pub name: String,
    pub snapshot_count: i64,
    /// Bytes of blobs referenced by this project (shared blobs count once per project).
    pub referenced_bytes: i64,
    pub budget_bytes: i64,
}

/// Content preview for the diff viewer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobContent {
    pub sha256: Option<String>,
    pub size: i64,
    pub binary: bool,
    pub truncated: bool,
    /// UTF-8 text (lossy) when not binary.
    pub text: Option<String>,
    pub source: String,
}

/// Progress update emitted during long operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub task_id: String,
    pub phase: String,
    pub done: u64,
    pub total: u64,
    pub current: Option<String>,
}

pub trait ProgressSink: Send + Sync {
    fn report(&self, p: Progress);
}

pub struct NoProgress;
impl ProgressSink for NoProgress {
    fn report(&self, _p: Progress) {}
}
