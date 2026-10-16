//! Worktree Intelligence: inventory and evidence-based diagnosis.
//!
//! A worktree whose path cannot be found is *not* assumed deleted: an
//! unmounted drive, a disconnected share and a moved directory all look the
//! same to a naive `exists()` check. The diagnosis records which of these the
//! evidence supports, and how confident it is.

use crate::error::{CoreError, Result};
use crate::git::{self, Git, StatusKind, WorktreeRecord};
use crate::paths;
use crate::platform::{self, Reachability};
use crate::store::path_key;
use crate::types::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Observation {
    pub record: WorktreeRecord,
    pub is_main: bool,
    pub diagnosis: WorktreeDiagnosis,
    pub evidence: Vec<String>,
    pub possible_new_path: Option<String>,
    pub counts: Option<StatusCounts>,
    pub dirty: Option<bool>,
    /// Digest of the dirty state; changes whenever the uncommitted work changes.
    pub fingerprint: Option<String>,
    pub status_error: Option<String>,
}

/// Cheap digest of the uncommitted state: status letters, staged object ids
/// and the size/mtime of every changed working-tree file.
pub fn dirty_fingerprint(wt: &Path, report: &git::StatusReport) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(report.head_oid.as_deref().unwrap_or("-"));
    for e in report
        .entries
        .iter()
        .filter(|e| e.kind != StatusKind::Ignored)
    {
        h.update(e.path.as_bytes());
        h.update([e.x as u8, e.y as u8]);
        h.update(e.oid_index.as_deref().unwrap_or("-"));
        if let Ok(p) = paths::safe_join(wt, e.path.trim_end_matches('/'))
            && let Ok(m) = std::fs::symlink_metadata(&p)
        {
            h.update(m.len().to_le_bytes());
            if let Ok(t) = m.modified()
                && let Ok(d) = t.duration_since(std::time::UNIX_EPOCH)
            {
                h.update(d.as_nanos().to_le_bytes());
            }
        }
        h.update([0]);
    }
    hex::encode(h.finalize())
}

/// Map of `path_key(worktree path)` -> admin directory name, read from
/// `<common>/worktrees/*/gitdir`.
fn admin_dirs(common_dir: &Path) -> HashMap<String, PathBuf> {
    let mut map = HashMap::new();
    let Ok(rd) = std::fs::read_dir(common_dir.join("worktrees")) else {
        return map;
    };
    for ent in rd.flatten() {
        let gitdir_file = ent.path().join("gitdir");
        if let Ok(content) = std::fs::read_to_string(&gitdir_file) {
            let p = git::native(content.trim());
            let wt = Path::new(&p)
                .parent()
                .map(|x| x.to_path_buf())
                .unwrap_or_default();
            map.insert(path_key(&paths::display(&wt)), ent.path());
        }
    }
    map
}

/// Read the `gitdir:` pointer of a linked worktree's `.git` file.
fn read_git_pointer(dir: &Path) -> Option<PathBuf> {
    let content = std::fs::read_to_string(dir.join(".git")).ok()?;
    let line = content.lines().find(|l| l.starts_with("gitdir:"))?;
    let raw = line.trim_start_matches("gitdir:").trim();
    let p = PathBuf::from(git::native(raw));
    Some(if p.is_absolute() { p } else { dir.join(p) })
}

/// Look for a directory near `missing` whose `.git` file points to `admin`.
fn find_relocation(missing: &Path, admin: &Path) -> Option<PathBuf> {
    let parent = missing.parent()?;
    let mut candidates = vec![parent.to_path_buf()];
    if let Some(gp) = parent.parent() {
        candidates.push(gp.to_path_buf());
    }
    for base in candidates {
        let Ok(rd) = std::fs::read_dir(&base) else {
            continue;
        };
        for ent in rd.flatten().take(500) {
            let p = ent.path();
            let Ok(meta) = std::fs::symlink_metadata(&p) else {
                continue;
            };
            if !meta.is_dir() || paths::is_link_like(&meta) {
                continue;
            }
            if let Some(ptr) = read_git_pointer(&p)
                && paths::same_path(
                    &paths::lexical_normalize(&ptr),
                    &paths::lexical_normalize(admin),
                )
            {
                return Some(p);
            }
        }
    }
    None
}

pub fn status_counts(report: &git::StatusReport) -> StatusCounts {
    StatusCounts {
        staged: report.staged_count() as i64,
        unstaged: report.unstaged_count() as i64,
        untracked: report.untracked_count() as i64,
        conflicted: report.conflicted_count() as i64,
        deleted: report
            .entries
            .iter()
            .filter(|e| {
                matches!(e.kind, StatusKind::Ordinary | StatusKind::Renamed)
                    && (e.x == 'D' || e.y == 'D')
            })
            .count() as i64,
        tracked_paths: 0,
    }
}

/// Count tracked paths (used only to put mass deletions in proportion).
pub fn tracked_count(git: &Git, wt: &Path) -> Option<i64> {
    let out = git.run(wt, &["ls-files", "-z"]).ok()?;
    Some(out.split(|b| *b == 0).filter(|s| !s.is_empty()).count() as i64)
}

/// Inventory every worktree Git knows about, plus diagnosis.
pub fn inventory(
    git: &Git,
    common_dir: &Path,
    deletion_threshold: i64,
) -> Result<Vec<Observation>> {
    if !common_dir.is_dir() {
        return Err(CoreError::NotARepository(common_dir.to_path_buf()));
    }
    let records = git::worktree_list(git, common_dir)?;
    let admins = admin_dirs(common_dir);
    let mut out = Vec::new();
    for (i, rec) in records.into_iter().enumerate() {
        if rec.bare {
            continue;
        }
        let is_main = i == 0;
        let path = PathBuf::from(&rec.path);
        let mut evidence = Vec::new();
        let mut possible_new_path = None;
        let mut counts = None;
        let mut dirty = None;
        let mut fingerprint = None;
        let mut status_error = None;
        if rec.prunable {
            evidence.push("gitReportsPrunable".to_string());
        }
        if rec.locked {
            evidence.push("locked".to_string());
        }
        let diagnosis = match platform::reachability(&path) {
            Reachability::Present => {
                let git_marker = path.join(".git");
                let pointer_ok = if is_main {
                    git_marker.exists()
                } else {
                    match (read_git_pointer(&path), admins.get(&path_key(&rec.path))) {
                        (Some(ptr), Some(admin)) => paths::same_path(
                            &paths::lexical_normalize(&ptr),
                            &paths::lexical_normalize(admin),
                        ),
                        (Some(_), None) => true,
                        _ => false,
                    }
                };
                if !pointer_ok {
                    evidence.push("gitPointerMismatch".to_string());
                    WorktreeDiagnosis::Unknown
                } else {
                    match git::status(git, &path, false, None) {
                        Ok(report) => {
                            let mut c = status_counts(&report);
                            if c.deleted >= deletion_threshold.max(1) {
                                c.tracked_paths = tracked_count(git, &path).unwrap_or(0);
                            }
                            dirty = Some(report.is_dirty());
                            fingerprint = Some(dirty_fingerprint(&path, &report));
                            counts = Some(c);
                            WorktreeDiagnosis::Healthy
                        }
                        Err(e) => {
                            evidence.push("statusFailed".to_string());
                            status_error = Some(e.to_string());
                            WorktreeDiagnosis::Inaccessible
                        }
                    }
                }
            }
            Reachability::MissingOnVolume => {
                evidence.push("pathNotFound".to_string());
                evidence.push("volumeMounted".to_string());
                let relocation = admins
                    .get(&path_key(&rec.path))
                    .and_then(|admin| find_relocation(&path, admin));
                if let Some(newp) = relocation {
                    evidence.push("pointerFoundElsewhere".to_string());
                    possible_new_path = Some(paths::display(&newp));
                    WorktreeDiagnosis::PossiblyMoved
                } else if rec.prunable {
                    WorktreeDiagnosis::PrunableCandidate
                } else {
                    WorktreeDiagnosis::Missing
                }
            }
            Reachability::VolumeUnavailable => {
                evidence.push("pathNotFound".to_string());
                evidence.push("volumeUnavailable".to_string());
                WorktreeDiagnosis::VolumeUnavailable
            }
            Reachability::Inaccessible => {
                evidence.push("accessDenied".to_string());
                WorktreeDiagnosis::Inaccessible
            }
        };
        out.push(Observation {
            record: rec,
            is_main,
            diagnosis,
            evidence,
            possible_new_path,
            counts,
            dirty,
            fingerprint,
            status_error,
        });
    }
    Ok(out)
}

/// Diagnose a worktree that is in our records but no longer listed by Git
/// (its metadata was pruned or removed).
pub fn diagnose_unlisted(path: &str) -> (WorktreeDiagnosis, Vec<String>) {
    let mut ev = vec!["removedFromGitMetadata".to_string()];
    match platform::reachability(Path::new(path)) {
        Reachability::Present => {
            ev.push("directoryStillExists".into());
            (WorktreeDiagnosis::Unknown, ev)
        }
        Reachability::MissingOnVolume => {
            ev.push("pathNotFound".into());
            (WorktreeDiagnosis::Missing, ev)
        }
        Reachability::VolumeUnavailable => {
            ev.push("volumeUnavailable".into());
            (WorktreeDiagnosis::VolumeUnavailable, ev)
        }
        Reachability::Inaccessible => {
            ev.push("accessDenied".into());
            (WorktreeDiagnosis::Inaccessible, ev)
        }
    }
}

/// Read-only preview of what `git worktree repair` / `prune` would need,
/// presented as guidance. Nothing here executes those commands.
pub fn repair_guidance(w: &Worktree) -> Vec<String> {
    let mut steps = Vec::new();
    match w.diagnosis {
        WorktreeDiagnosis::PossiblyMoved => {
            if let Some(newp) = &w.possible_new_path {
                steps.push(format!("git worktree repair \"{newp}\""));
            }
        }
        WorktreeDiagnosis::VolumeUnavailable => {
            steps.push("reconnect".into());
        }
        WorktreeDiagnosis::PrunableCandidate | WorktreeDiagnosis::Missing => {
            if !w.locked {
                steps.push(format!(
                    "git worktree lock --reason \"kept by Worktree Rescue\" \"{}\"",
                    w.path
                ));
            }
            steps.push("git worktree prune --dry-run --verbose".into());
        }
        _ => {}
    }
    steps
}
