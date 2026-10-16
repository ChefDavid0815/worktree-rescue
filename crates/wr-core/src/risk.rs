//! Risk Observatory: a local rule engine over observed facts.
//!
//! Rules only *describe* situations; they never block Git or agents, and
//! they never attribute changes to a specific tool. Every risk carries the
//! raw evidence it was derived from.

use crate::store::path_key;
use crate::types::*;
use serde_json::json;

pub mod rules {
    pub const WORK_AT_RISK: &str = "unprotectedWorkUnreachable";
    pub const UNREACHABLE_PROTECTED: &str = "worktreeUnreachableProtected";
    pub const UNREACHABLE_CLEAN: &str = "worktreeUnreachable";
    pub const STALE_PROTECTION: &str = "staleProtection";
    pub const AUTO_FAILING: &str = "autoProtectFailing";
    pub const STORAGE_LOW: &str = "storageLow";
    pub const BUDGET_NEARLY_FULL: &str = "budgetNearlyFull";
    pub const UNPROTECTED_FILES: &str = "unprotectedFiles";
    pub const MASS_DELETION: &str = "massDeletion";
    pub const INTEGRITY: &str = "snapshotIntegrity";

    /// Rules re-evaluated on every inventory pass (and auto-resolved when
    /// their condition clears).
    pub const EVALUATED: &[&str] = &[
        WORK_AT_RISK,
        UNREACHABLE_PROTECTED,
        UNREACHABLE_CLEAN,
        STALE_PROTECTION,
        AUTO_FAILING,
        STORAGE_LOW,
        BUDGET_NEARLY_FULL,
        UNPROTECTED_FILES,
        MASS_DELETION,
    ];
}

/// Facts about one worktree gathered by the vault before evaluation.
pub struct WorktreeFacts<'a> {
    pub worktree: &'a Worktree,
    pub consecutive_auto_failures: i64,
    /// Skip reasons (and counts) from the latest snapshot of this worktree.
    pub latest_skips: Vec<(String, i64)>,
}

pub struct ProjectFacts<'a> {
    pub project: &'a Project,
    pub worktrees: Vec<WorktreeFacts<'a>>,
    pub available_bytes: Option<u64>,
    pub min_free_bytes: i64,
    pub referenced_bytes: i64,
    pub budget_bytes: i64,
    pub stale_minutes: i64,
    pub mass_deletion_threshold: i64,
    pub now: i64,
}

pub struct Finding {
    pub risk: Risk,
    pub dedupe_key: String,
}

fn finding(
    p: &Project,
    wt: Option<&str>,
    rule: &str,
    severity: Severity,
    confidence: Confidence,
    now: i64,
    evidence: serde_json::Value,
) -> Finding {
    let key = format!("{rule}|{}", wt.map(path_key).unwrap_or_default());
    Finding {
        risk: Risk {
            id: uuid::Uuid::now_v7().to_string(),
            project_id: p.id.clone(),
            worktree_path: wt.map(str::to_string),
            rule: rule.to_string(),
            severity,
            confidence,
            detected_at: now,
            last_seen_at: now,
            resolved_at: None,
            acknowledged: false,
            evidence,
        },
        dedupe_key: key,
    }
}

pub fn evaluate(f: &ProjectFacts<'_>) -> Vec<Finding> {
    let mut out = Vec::new();
    let p = f.project;
    let now = f.now;

    for wf in &f.worktrees {
        let w = wf.worktree;
        let path = Some(w.path.as_str());
        let unreachable = !matches!(
            w.diagnosis,
            WorktreeDiagnosis::Healthy | WorktreeDiagnosis::Inaccessible
        ) || w.gone_from_git;

        if unreachable && !(w.diagnosis == WorktreeDiagnosis::Unknown && !w.gone_from_git) {
            let had_unsaved = match (w.last_dirty_at, w.last_protected_at) {
                (Some(d), Some(s)) => d > s,
                (Some(_), None) => true,
                _ => false,
            };
            let ev = json!({
                "diagnosis": w.diagnosis.as_str(),
                "evidence": w.evidence,
                "lastSeenAt": w.last_seen_at,
                "lastDirtyAt": w.last_dirty_at,
                "lastSnapshotAt": w.last_snapshot_at,
                "lastSnapshotId": w.last_snapshot_id,
                "possibleNewPath": w.possible_new_path,
                "branch": w.branch,
                "locked": w.locked,
                "lastCounts": w.counts,
            });
            let confidence = match w.diagnosis {
                WorktreeDiagnosis::Missing | WorktreeDiagnosis::PrunableCandidate => {
                    Confidence::High
                }
                WorktreeDiagnosis::PossiblyMoved => Confidence::Medium,
                _ => Confidence::Low,
            };
            if had_unsaved {
                // An unavailable volume is serious but the data may be fine.
                let sev = if w.diagnosis == WorktreeDiagnosis::VolumeUnavailable {
                    Severity::Attention
                } else {
                    Severity::Critical
                };
                out.push(finding(
                    p,
                    path,
                    rules::WORK_AT_RISK,
                    sev,
                    confidence,
                    now,
                    ev,
                ));
            } else if w.last_protected_at.is_some() && w.last_dirty_at.is_some() {
                out.push(finding(
                    p,
                    path,
                    rules::UNREACHABLE_PROTECTED,
                    Severity::Attention,
                    confidence,
                    now,
                    ev,
                ));
            } else {
                out.push(finding(
                    p,
                    path,
                    rules::UNREACHABLE_CLEAN,
                    Severity::Info,
                    confidence,
                    now,
                    ev,
                ));
            }
            continue;
        }

        if w.diagnosis == WorktreeDiagnosis::Healthy && w.dirty == Some(true) {
            let stale_ms = f.stale_minutes.max(1) * 60_000;
            let since = w.last_protected_at;
            let stale = match since {
                None => {
                    w.last_dirty_at.is_some_and(|d| now - d >= stale_ms)
                        || w.first_seen_at + stale_ms <= now
                }
                Some(s) => now - s >= stale_ms && w.last_dirty_at.is_some_and(|d| d > s),
            };
            if stale {
                out.push(finding(
                    p,
                    path,
                    rules::STALE_PROTECTION,
                    Severity::Attention,
                    Confidence::High,
                    now,
                    json!({
                        "lastSnapshotAt": since,
                        "minutesWithoutSnapshot": since.map(|s| (now - s) / 60_000),
                        "thresholdMinutes": f.stale_minutes,
                        "counts": w.counts,
                        "autoProtect": p.auto_protect,
                    }),
                ));
            }
        }

        if let Some(c) = &w.counts
            && w.diagnosis == WorktreeDiagnosis::Healthy
            && c.deleted >= f.mass_deletion_threshold.max(1)
        {
            let fraction = if c.tracked_paths > 0 {
                c.deleted as f64 / c.tracked_paths as f64
            } else {
                0.0
            };
            let severe = fraction >= 0.3 || c.deleted >= f.mass_deletion_threshold * 4;
            out.push(finding(
                p,
                path,
                rules::MASS_DELETION,
                if severe {
                    Severity::Critical
                } else {
                    Severity::Attention
                },
                Confidence::High,
                now,
                json!({
                    "deleted": c.deleted,
                    "trackedPaths": c.tracked_paths,
                    "fraction": (fraction * 1000.0).round() / 1000.0,
                    "threshold": f.mass_deletion_threshold,
                    "lastSnapshotAt": w.last_snapshot_at,
                    "lastSnapshotId": w.last_snapshot_id,
                }),
            ));
        }

        if p.auto_protect && wf.consecutive_auto_failures >= 3 {
            out.push(finding(
                p,
                path,
                rules::AUTO_FAILING,
                if wf.consecutive_auto_failures >= 6 {
                    Severity::Critical
                } else {
                    Severity::Attention
                },
                Confidence::High,
                now,
                json!({ "consecutiveFailures": wf.consecutive_auto_failures }),
            ));
        }

        let notable: Vec<&(String, i64)> = wf
            .latest_skips
            .iter()
            .filter(|(r, _)| {
                matches!(
                    r.as_str(),
                    "tooLarge" | "sensitive" | "budgetExceeded" | "unreadable" | "unstable"
                )
            })
            .collect();
        if !notable.is_empty() {
            let severe = notable
                .iter()
                .any(|(r, _)| matches!(r.as_str(), "budgetExceeded" | "unreadable" | "unstable"));
            let reasons: serde_json::Map<String, serde_json::Value> =
                notable.iter().map(|(r, n)| (r.clone(), json!(n))).collect();
            out.push(finding(
                p,
                path,
                rules::UNPROTECTED_FILES,
                if severe {
                    Severity::Attention
                } else {
                    Severity::Info
                },
                Confidence::High,
                now,
                json!({ "reasons": reasons, "snapshotId": w.last_snapshot_id }),
            ));
        }
    }

    if let Some(avail) = f.available_bytes
        && (avail as i64) < f.min_free_bytes
    {
        out.push(finding(
            p,
            None,
            rules::STORAGE_LOW,
            if (avail as i64) < f.min_free_bytes / 4 {
                Severity::Critical
            } else {
                Severity::Attention
            },
            Confidence::High,
            now,
            json!({ "availableBytes": avail, "minimumBytes": f.min_free_bytes }),
        ));
    }
    if f.budget_bytes > 0 && f.referenced_bytes * 10 >= f.budget_bytes * 9 {
        out.push(finding(
            p,
            None,
            rules::BUDGET_NEARLY_FULL,
            if f.referenced_bytes >= f.budget_bytes {
                Severity::Critical
            } else {
                Severity::Attention
            },
            Confidence::High,
            now,
            json!({ "usedBytes": f.referenced_bytes, "budgetBytes": f.budget_bytes }),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> Project {
        Project {
            id: "p".into(),
            name: "p".into(),
            root_path: "/r".into(),
            common_dir: "/r/.git".into(),
            is_demo: false,
            added_at: 0,
            auto_protect: true,
            monitoring: Monitoring::Active,
            last_snapshot_at: None,
            last_verified_snapshot_at: None,
            snapshot_count: 0,
            stored_bytes: 0,
            open_risks: RiskCounts::default(),
        }
    }

    fn wt(diag: WorktreeDiagnosis) -> Worktree {
        Worktree {
            id: "w".into(),
            project_id: "p".into(),
            path: "/r/wt".into(),
            is_main: false,
            branch: Some("feat".into()),
            head_oid: None,
            detached: false,
            locked: false,
            lock_reason: None,
            git_prunable: false,
            prunable_reason: None,
            diagnosis: diag,
            evidence: vec![],
            possible_new_path: None,
            dirty: Some(true),
            counts: None,
            first_seen_at: 0,
            last_seen_at: Some(1000),
            last_checked_at: 2000,
            last_dirty_at: Some(1000),
            dirty_fingerprint: None,
            last_confirmed_at: None,
            last_protected_at: None,
            last_snapshot_id: None,
            last_snapshot_at: None,
            last_verified_snapshot_at: None,
            gone_from_git: false,
        }
    }

    fn facts<'a>(p: &'a Project, ws: Vec<WorktreeFacts<'a>>) -> ProjectFacts<'a> {
        ProjectFacts {
            project: p,
            worktrees: ws,
            available_bytes: Some(u64::MAX / 4),
            min_free_bytes: 1,
            referenced_bytes: 0,
            budget_bytes: 100,
            stale_minutes: 120,
            mass_deletion_threshold: 25,
            now: 10_000,
        }
    }

    #[test]
    fn missing_dirty_worktree_is_critical() {
        let p = project();
        let w = wt(WorktreeDiagnosis::Missing);
        let r = evaluate(&facts(
            &p,
            vec![WorktreeFacts {
                worktree: &w,
                consecutive_auto_failures: 0,
                latest_skips: vec![],
            }],
        ));
        assert_eq!(r[0].risk.rule, rules::WORK_AT_RISK);
        assert_eq!(r[0].risk.severity, Severity::Critical);
    }

    #[test]
    fn unavailable_volume_is_not_critical() {
        let p = project();
        let w = wt(WorktreeDiagnosis::VolumeUnavailable);
        let r = evaluate(&facts(
            &p,
            vec![WorktreeFacts {
                worktree: &w,
                consecutive_auto_failures: 0,
                latest_skips: vec![],
            }],
        ));
        assert_eq!(r[0].risk.severity, Severity::Attention);
        assert_eq!(r[0].risk.confidence, Confidence::Low);
    }

    #[test]
    fn protected_missing_worktree_is_attention() {
        let p = project();
        let mut w = wt(WorktreeDiagnosis::Missing);
        w.last_snapshot_at = Some(1500);
        w.last_protected_at = Some(1500);
        let r = evaluate(&facts(
            &p,
            vec![WorktreeFacts {
                worktree: &w,
                consecutive_auto_failures: 0,
                latest_skips: vec![],
            }],
        ));
        assert_eq!(r[0].risk.rule, rules::UNREACHABLE_PROTECTED);
    }

    #[test]
    fn mass_deletion_and_failures() {
        let p = project();
        let mut w = wt(WorktreeDiagnosis::Healthy);
        w.last_snapshot_at = Some(9_999);
        w.last_protected_at = Some(9_999);
        w.counts = Some(StatusCounts {
            deleted: 40,
            tracked_paths: 100,
            ..Default::default()
        });
        let r = evaluate(&facts(
            &p,
            vec![WorktreeFacts {
                worktree: &w,
                consecutive_auto_failures: 4,
                latest_skips: vec![("tooLarge".into(), 2)],
            }],
        ));
        let rules_found: Vec<&str> = r.iter().map(|f| f.risk.rule.as_str()).collect();
        assert!(rules_found.contains(&rules::MASS_DELETION));
        assert!(rules_found.contains(&rules::AUTO_FAILING));
        assert!(rules_found.contains(&rules::UNPROTECTED_FILES));
        let md = r
            .iter()
            .find(|f| f.risk.rule == rules::MASS_DELETION)
            .unwrap();
        assert_eq!(md.risk.severity, Severity::Critical);
    }
}
