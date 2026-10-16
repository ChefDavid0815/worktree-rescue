//! Timeline event kinds. The UI localises each kind; `data` carries the
//! parameters (old/new branch, counts, snapshot ids…).

pub const PROJECT_ADDED: &str = "projectAdded";
pub const SNAPSHOT_CREATED: &str = "snapshotCreated";
pub const SNAPSHOT_PARTIAL: &str = "snapshotPartial";
pub const SNAPSHOT_FAILED: &str = "snapshotFailed";
pub const SNAPSHOT_INTERRUPTED: &str = "snapshotInterrupted";
pub const SNAPSHOT_DELETED: &str = "snapshotsPruned";
pub const BRANCH_CHANGED: &str = "branchChanged";
pub const HEAD_MOVED: &str = "headMoved";
pub const WORKTREE_ADDED: &str = "worktreeAdded";
pub const WORKTREE_UNREACHABLE: &str = "worktreeUnreachable";
pub const WORKTREE_REAPPEARED: &str = "worktreeReappeared";
pub const WORKTREE_REMOVED_FROM_GIT: &str = "worktreeRemovedFromGit";
pub const WORKTREE_LOCKED: &str = "worktreeLocked";
pub const WORKTREE_UNLOCKED: &str = "worktreeUnlocked";
pub const BECAME_DIRTY: &str = "workingTreeChanged";
pub const BECAME_CLEAN: &str = "workingTreeClean";
pub const RISK_RAISED: &str = "riskRaised";
pub const RISK_RESOLVED: &str = "riskResolved";
pub const RECOVERY_COMPLETED: &str = "recoveryCompleted";
pub const RECOVERY_FAILED: &str = "recoveryFailed";
pub const MONITORING_CHANGED: &str = "monitoringChanged";
pub const AUTO_PROTECT_CHANGED: &str = "autoProtectChanged";
pub const INTEGRITY_CHECKED: &str = "integrityChecked";
