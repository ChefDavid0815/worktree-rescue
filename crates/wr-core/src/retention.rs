//! Retention policy evaluation. Pure function over snapshot summaries so the
//! rules can be unit tested; deletion itself happens in the vault after
//! reference checks.

use crate::settings::RetentionPolicy;
use crate::store::path_key;
use crate::types::{SnapshotStatus, SnapshotSummary};
use std::collections::{HashMap, HashSet};

const HOUR: i64 = 3_600_000;
const DAY: i64 = 24 * HOUR;

/// Return the ids that the policy would prune.
pub fn plan(snapshots: &[SnapshotSummary], policy: &RetentionPolicy, now: i64) -> Vec<String> {
    let mut groups: HashMap<String, Vec<&SnapshotSummary>> = HashMap::new();
    for s in snapshots {
        groups
            .entry(path_key(&s.worktree_path))
            .or_default()
            .push(s);
    }
    let mut prune = Vec::new();
    for (_, mut list) in groups {
        list.sort_by_key(|s| std::cmp::Reverse(s.created_at)); // newest first
        let latest_recoverable = list
            .iter()
            .find(|s| s.status.is_recoverable())
            .map(|s| s.id.clone());
        let mut kept: Vec<&SnapshotSummary> = Vec::new();
        let mut hour_buckets = HashSet::new();
        let mut day_buckets = HashSet::new();
        for s in &list {
            let age = now - s.created_at;
            let protected = s.pinned
                || Some(&s.id) == latest_recoverable.as_ref()
                || s.status == SnapshotStatus::InProgress;
            if protected {
                kept.push(s);
                continue;
            }
            if !s.status.is_recoverable() {
                // Failed / interrupted records hold no recoverable data.
                if age > policy.keep_all_hours * HOUR {
                    prune.push(s.id.clone());
                } else {
                    kept.push(s);
                }
                continue;
            }
            if age <= policy.keep_all_hours * HOUR {
                kept.push(s);
            } else if age <= policy.keep_all_hours * HOUR + policy.keep_hourly_days * DAY {
                if hour_buckets.insert(s.created_at / HOUR) {
                    kept.push(s);
                } else {
                    prune.push(s.id.clone());
                }
            } else if age
                <= policy.keep_all_hours * HOUR
                    + policy.keep_hourly_days * DAY
                    + policy.keep_daily_days * DAY
            {
                if day_buckets.insert(s.created_at / DAY) {
                    kept.push(s);
                } else {
                    prune.push(s.id.clone());
                }
            } else {
                prune.push(s.id.clone());
            }
        }
        // Hard cap: drop the oldest unprotected kept snapshots.
        let cap = policy.max_snapshots_per_worktree.max(1) as usize;
        if kept.len() > cap {
            let mut excess = kept.len() - cap;
            for s in kept.iter().rev() {
                if excess == 0 {
                    break;
                }
                if s.pinned
                    || Some(&s.id) == latest_recoverable.as_ref()
                    || s.status == SnapshotStatus::InProgress
                {
                    continue;
                }
                prune.push(s.id.clone());
                excess -= 1;
            }
        }
    }
    prune.sort();
    prune.dedup();
    prune
}

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GcReport {
    pub snapshots_pruned: usize,
    pub blobs_deleted: usize,
    pub bytes_freed: u64,
    pub orphan_files_deleted: usize,
    pub temp_files_deleted: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    fn snap(id: &str, at: i64, status: SnapshotStatus, pinned: bool) -> SnapshotSummary {
        SnapshotSummary {
            id: id.into(),
            project_id: "p".into(),
            worktree_path: "/w".into(),
            created_at: at,
            finished_at: Some(at),
            trigger: SnapshotTrigger::Auto,
            status,
            branch: None,
            head_oid: None,
            detached: false,
            entry_count: 1,
            captured_count: 1,
            skipped_count: 0,
            failed_count: 0,
            staged_count: 0,
            unstaged_count: 1,
            untracked_count: 0,
            deleted_count: 0,
            protected_bytes: 1,
            new_bytes: 1,
            manifest_sha256: None,
            pinned,
            note: None,
            error_code: None,
            error_message: None,
            verified_at: None,
        }
    }

    #[test]
    fn keeps_recent_thins_old_and_respects_pins() {
        let now = 100 * DAY;
        let policy = RetentionPolicy {
            keep_all_hours: 24,
            keep_hourly_days: 1,
            keep_daily_days: 2,
            max_snapshots_per_worktree: 100,
        };
        let v = vec![
            snap("recent1", now - HOUR, SnapshotStatus::Verified, false),
            snap("recent2", now - 2 * HOUR, SnapshotStatus::Verified, false),
            // Two in the same hour, 30h ago: one pruned.
            snap(
                "h1",
                now - 30 * HOUR - 600_000,
                SnapshotStatus::Verified,
                false,
            ),
            snap(
                "h2",
                now - 30 * HOUR - 1_200_000,
                SnapshotStatus::Verified,
                false,
            ),
            // Ancient: pruned unless pinned.
            snap("old", now - 50 * DAY, SnapshotStatus::Verified, false),
            snap("oldpinned", now - 60 * DAY, SnapshotStatus::Verified, true),
            snap("failed", now - 3 * DAY, SnapshotStatus::Failed, false),
        ];
        let p = plan(&v, &policy, now);
        assert!(p.contains(&"h2".to_string()));
        assert!(!p.contains(&"h1".to_string()));
        assert!(p.contains(&"old".to_string()));
        assert!(p.contains(&"failed".to_string()));
        assert!(!p.contains(&"oldpinned".to_string()));
        assert!(!p.contains(&"recent1".to_string()));
    }

    #[test]
    fn never_prunes_latest_recoverable() {
        let now = 100 * DAY;
        let v = vec![snap(
            "only",
            now - 90 * DAY,
            SnapshotStatus::Verified,
            false,
        )];
        assert!(plan(&v, &RetentionPolicy::default(), now).is_empty());
    }
}
