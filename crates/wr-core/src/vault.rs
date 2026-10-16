//! The vault: one data root (database, blob store, manifests) plus the
//! high-level operations the application exposes. The production vault and
//! the demo vault are separate instances with separate roots, so demo data
//! can never mix with real project data.

use crate::blob::BlobStore;
use crate::error::{CoreError, Result};
use crate::events as ev;
use crate::filter::CapturePolicy;
use crate::forensics::{self, ForensicCandidate, ForensicReport};
use crate::git::{self, Git};
use crate::manifest::{self, Manifest};
use crate::paths;
use crate::recovery::{self, RecoveryReport, RestorePreview, Variant};
use crate::retention::{self, GcReport};
use crate::risk;
use crate::settings::Settings;
use crate::snapshot::{CaptureOutcome, CaptureRequest, Engine};
use crate::store::{Store, path_key};
use crate::types::*;
use crate::worktree;
use serde::Serialize;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, RwLock};

pub struct Vault {
    root: PathBuf,
    pub is_demo: bool,
    store: Mutex<Store>,
    blobs: BlobStore,
    manifests: PathBuf,
    git: RwLock<Option<Git>>,
    settings: RwLock<Settings>,
    busy: Mutex<HashSet<String>>,
    /// Captures hold a read guard; garbage collection takes the write guard.
    gc_gate: RwLock<()>,
    previews: Mutex<HashMap<String, RestorePreview>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshResult {
    pub worktrees: Vec<Worktree>,
    pub new_risks: Vec<Risk>,
    pub resolved_risks: Vec<Risk>,
    pub events: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotDetail {
    pub summary: SnapshotSummary,
    pub files: Vec<SnapshotFile>,
    pub git: Option<manifest::GitContext>,
    pub ignored_not_protected: u64,
    pub manifest_ok: bool,
    pub manifest_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyResult {
    pub snapshot_id: String,
    pub ok: bool,
    pub manifest_ok: bool,
    pub blobs_checked: usize,
    pub problems: Vec<(String, String)>,
    pub checked_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoProbe {
    pub identity: git::RepoIdentity,
    pub main_worktree: String,
    pub already_registered: Option<String>,
    pub worktree_count: usize,
    pub status: Option<StatusCounts>,
    pub support: Vec<SupportNote>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportNote {
    pub feature: String,
    /// supported | partial | unsupported
    pub level: String,
}

struct BusyGuard<'a> {
    set: &'a Mutex<HashSet<String>>,
    key: String,
}
impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.set.lock().unwrap().remove(&self.key);
    }
}

impl Vault {
    pub fn open(root: &Path, is_demo: bool) -> Result<Self> {
        std::fs::create_dir_all(root).map_err(|e| CoreError::io(root, e))?;
        let store = Store::open(&root.join("vault.db"))?;
        let interrupted = store.mark_interrupted()?;
        if interrupted > 0 {
            log::warn!("{interrupted} snapshot(s) were interrupted by a previous shutdown");
        }
        let settings = store.load_settings()?;
        let blobs = BlobStore::open(root.join("blobs"))?;
        blobs.clean_tmp();
        let manifests = root.join("manifests");
        std::fs::create_dir_all(&manifests).map_err(|e| CoreError::io(&manifests, e))?;
        let git = Git::discover(settings.git_path.as_deref().map(Path::new)).ok();
        Ok(Vault {
            root: root.to_path_buf(),
            is_demo,
            store: Mutex::new(store),
            blobs,
            manifests,
            git: RwLock::new(git),
            settings: RwLock::new(settings),
            busy: Mutex::new(HashSet::new()),
            gc_gate: RwLock::new(()),
            previews: Mutex::new(HashMap::new()),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn git(&self) -> Result<Git> {
        self.git
            .read()
            .unwrap()
            .clone()
            .ok_or(CoreError::GitNotFound)
    }

    pub fn git_opt(&self) -> Option<Git> {
        self.git.read().unwrap().clone()
    }

    pub fn store(&self) -> std::sync::MutexGuard<'_, Store> {
        self.store.lock().unwrap()
    }

    pub fn blobs(&self) -> &BlobStore {
        &self.blobs
    }

    // ------------------------------------------------------------------
    // Settings
    // ------------------------------------------------------------------

    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }

    pub fn update_settings(&self, s: Settings) -> Result<Settings> {
        s.validate()?;
        CapturePolicy::from_settings(&s)?;
        self.store().save_settings(&s)?;
        let git = Git::discover(s.git_path.as_deref().map(Path::new)).ok();
        *self.git.write().unwrap() = git;
        *self.settings.write().unwrap() = s.clone();
        Ok(s)
    }

    /// Shared settings are owned by the main vault; the demo vault mirrors them.
    pub fn mirror_settings(&self, s: &Settings) {
        *self.settings.write().unwrap() = s.clone();
        let git = Git::discover(s.git_path.as_deref().map(Path::new)).ok();
        *self.git.write().unwrap() = git;
    }

    // ------------------------------------------------------------------
    // Projects
    // ------------------------------------------------------------------

    pub fn probe_repository(&self, path: &Path) -> Result<RepoProbe> {
        let git = self.git()?;
        let identity = git::identify(&git, path)?;
        let list = git::worktree_list(&git, Path::new(&identity.toplevel))?;
        let main = list
            .first()
            .map(|w| w.path.clone())
            .unwrap_or_else(|| identity.toplevel.clone());
        let already = self
            .store()
            .find_project_by_common_dir(&identity.common_dir)?;
        let status = git::status(&git, Path::new(&identity.toplevel), false, None)
            .ok()
            .map(|r| worktree::status_counts(&r));
        let mut support = vec![
            SupportNote {
                feature: "worktrees".into(),
                level: "supported".into(),
            },
            SupportNote {
                feature: "detachedHead".into(),
                level: "supported".into(),
            },
            SupportNote {
                feature: "unbornBranch".into(),
                level: "supported".into(),
            },
        ];
        if identity.has_submodules {
            support.push(SupportNote {
                feature: "submodules".into(),
                level: "partial".into(),
            });
        }
        if identity.sparse_checkout {
            support.push(SupportNote {
                feature: "sparseCheckout".into(),
                level: "partial".into(),
            });
        }
        if identity.uses_lfs {
            support.push(SupportNote {
                feature: "lfs".into(),
                level: "partial".into(),
            });
        }
        Ok(RepoProbe {
            identity,
            main_worktree: main,
            already_registered: already,
            worktree_count: list.iter().filter(|w| !w.bare).count(),
            status,
            support,
        })
    }

    pub fn add_project(&self, path: &Path, auto_protect: Option<bool>) -> Result<Project> {
        let probe = self.probe_repository(path)?;
        if probe.already_registered.is_some() {
            return Err(CoreError::AlreadyRegistered(probe.main_worktree));
        }
        let name = Path::new(&probe.main_worktree)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "repository".into());
        let id = uuid::Uuid::now_v7().to_string();
        let auto = auto_protect.unwrap_or(self.settings().auto_protect_new_projects);
        {
            let store = self.store();
            store.insert_project(
                &id,
                &name,
                &probe.main_worktree,
                &probe.identity.common_dir,
                self.is_demo,
                auto,
            )?;
            store.add_event(
                &id,
                Some(&probe.main_worktree),
                ev::PROJECT_ADDED,
                None,
                json!({ "autoProtect": auto, "worktrees": probe.worktree_count }),
            )?;
        }
        let _ = self.refresh_project(&id);
        self.store().get_project(&id)
    }

    pub fn list_projects(&self) -> Result<Vec<Project>> {
        self.store().list_projects()
    }

    pub fn has_project(&self, id: &str) -> bool {
        self.store().get_project(id).is_ok()
    }

    pub fn set_auto_protect(&self, id: &str, on: bool) -> Result<()> {
        let store = self.store();
        store.set_project_auto_protect(id, on)?;
        store.add_event(
            id,
            None,
            ev::AUTO_PROTECT_CHANGED,
            None,
            json!({ "enabled": on }),
        )?;
        Ok(())
    }

    pub fn set_monitoring(&self, id: &str, m: Monitoring) -> Result<()> {
        let store = self.store();
        store.set_project_monitoring(id, m)?;
        store.add_event(
            id,
            None,
            ev::MONITORING_CHANGED,
            None,
            json!({ "monitoring": m.as_str() }),
        )?;
        Ok(())
    }

    /// Stop monitoring a project. When `delete_backups` is false the
    /// snapshots stay in the vault and the project remains browsable but
    /// paused; when true every snapshot and unreferenced blob is deleted.
    pub fn remove_project(&self, id: &str, delete_backups: bool) -> Result<GcReport> {
        if !delete_backups {
            self.set_monitoring(id, Monitoring::Paused)?;
            self.set_auto_protect(id, false)?;
            return Ok(GcReport::default());
        }
        let _gate = self.gc_gate.write().unwrap();
        let snapshot_ids = self.store.lock().unwrap().delete_project(id)?;
        for sid in &snapshot_ids {
            let _ = manifest::delete_manifest(&self.manifests, sid);
        }
        let mut report = self.collect_blobs()?;
        report.snapshots_pruned = snapshot_ids.len();
        Ok(report)
    }

    // ------------------------------------------------------------------
    // Inventory & risk
    // ------------------------------------------------------------------

    pub fn list_worktrees(&self, project_id: &str) -> Result<Vec<Worktree>> {
        self.store().list_worktrees(project_id)
    }

    pub fn refresh_project(&self, project_id: &str) -> Result<RefreshResult> {
        let git = self.git()?;
        let project = self.store().get_project(project_id)?;
        let settings = self.settings();
        let observations = worktree::inventory(
            &git,
            Path::new(&project.common_dir),
            settings.mass_deletion_threshold,
        )?;
        let now = crate::now_ms();
        let prev = self.store().list_worktrees(project_id)?;
        let prev_map: HashMap<String, &Worktree> =
            prev.iter().map(|w| (path_key(&w.path), w)).collect();
        let mut events: Vec<(Option<String>, &'static str, serde_json::Value)> = Vec::new();
        let mut seen = HashSet::new();
        let mut updated = Vec::new();
        for o in &observations {
            let key = path_key(&o.record.path);
            seen.insert(key.clone());
            let p = prev_map.get(&key).copied();
            let healthy = o.diagnosis == WorktreeDiagnosis::Healthy;
            let w = Worktree {
                id: p
                    .map(|x| x.id.clone())
                    .unwrap_or_else(|| uuid::Uuid::now_v7().to_string()),
                project_id: project_id.to_string(),
                path: o.record.path.clone(),
                is_main: o.is_main,
                branch: o.record.branch.clone(),
                head_oid: o.record.head.clone(),
                detached: o.record.detached,
                locked: o.record.locked,
                lock_reason: o.record.lock_reason.clone(),
                git_prunable: o.record.prunable,
                prunable_reason: o.record.prunable_reason.clone(),
                diagnosis: o.diagnosis,
                evidence: o.evidence.clone(),
                possible_new_path: o.possible_new_path.clone(),
                dirty: o.dirty.or(p.and_then(|x| x.dirty)),
                counts: o.counts.clone().or(p.and_then(|x| x.counts.clone())),
                first_seen_at: p.map(|x| x.first_seen_at).unwrap_or(now),
                last_seen_at: if healthy {
                    Some(now)
                } else {
                    p.and_then(|x| x.last_seen_at)
                },
                last_checked_at: now,
                // Advance only when the dirty state actually changed.
                last_dirty_at: match (&o.fingerprint, p) {
                    (Some(fp), Some(prev))
                        if o.dirty == Some(true) && prev.dirty_fingerprint.as_ref() != Some(fp) =>
                    {
                        Some(now)
                    }
                    (Some(_), None) if o.dirty == Some(true) => Some(now),
                    _ => p.and_then(|x| x.last_dirty_at),
                },
                dirty_fingerprint: o
                    .fingerprint
                    .clone()
                    .or(p.and_then(|x| x.dirty_fingerprint.clone())),
                last_confirmed_at: p.and_then(|x| x.last_confirmed_at),
                last_protected_at: None,
                last_snapshot_id: None,
                last_snapshot_at: None,
                last_verified_snapshot_at: None,
                gone_from_git: false,
            };
            let wp = Some(w.path.clone());
            match p {
                None => events.push((
                    wp.clone(),
                    ev::WORKTREE_ADDED,
                    json!({ "branch": w.branch, "isMain": w.is_main }),
                )),
                Some(prev) => {
                    let was_healthy =
                        prev.diagnosis == WorktreeDiagnosis::Healthy && !prev.gone_from_git;
                    if was_healthy && !healthy {
                        events.push((wp.clone(), ev::WORKTREE_UNREACHABLE, json!({ "diagnosis": w.diagnosis.as_str(), "evidence": w.evidence, "possibleNewPath": w.possible_new_path })));
                    } else if !was_healthy && healthy {
                        events.push((
                            wp.clone(),
                            ev::WORKTREE_REAPPEARED,
                            json!({ "previous": prev.diagnosis.as_str() }),
                        ));
                    }
                    if healthy {
                        if prev.branch != w.branch {
                            events.push((
                                wp.clone(),
                                ev::BRANCH_CHANGED,
                                json!({ "from": prev.branch, "to": w.branch, "head": w.head_oid }),
                            ));
                        } else if prev.head_oid != w.head_oid {
                            events.push((wp.clone(), ev::HEAD_MOVED, json!({ "from": prev.head_oid, "to": w.head_oid, "branch": w.branch })));
                        }
                        if prev.dirty == Some(false) && w.dirty == Some(true) {
                            events.push((
                                wp.clone(),
                                ev::BECAME_DIRTY,
                                json!({ "counts": w.counts }),
                            ));
                        } else if prev.dirty == Some(true) && w.dirty == Some(false) {
                            events.push((wp.clone(), ev::BECAME_CLEAN, json!({})));
                        }
                    }
                    if !prev.locked && w.locked {
                        events.push((
                            wp.clone(),
                            ev::WORKTREE_LOCKED,
                            json!({ "reason": w.lock_reason }),
                        ));
                    } else if prev.locked && !w.locked {
                        events.push((wp.clone(), ev::WORKTREE_UNLOCKED, json!({})));
                    }
                }
            }
            updated.push(w);
        }
        for p in &prev {
            if seen.contains(&path_key(&p.path)) {
                continue;
            }
            let (diag, evidence) = worktree::diagnose_unlisted(&p.path);
            if !p.gone_from_git {
                events.push((
                    Some(p.path.clone()),
                    ev::WORKTREE_REMOVED_FROM_GIT,
                    json!({ "diagnosis": diag.as_str(), "branch": p.branch }),
                ));
            }
            let mut w = p.clone();
            w.diagnosis = diag;
            w.evidence = evidence;
            w.gone_from_git = true;
            w.last_checked_at = now;
            updated.push(w);
        }
        {
            let store = self.store();
            for w in &updated {
                store.upsert_worktree(w)?;
            }
            for (wp, kind, data) in &events {
                store.add_event(project_id, wp.as_deref(), kind, None, data.clone())?;
            }
        }
        let worktrees = self.store().list_worktrees(project_id)?;
        let (new_risks, resolved_risks) = self.evaluate_risks(&project, &worktrees, &settings)?;
        Ok(RefreshResult {
            worktrees: self.store().list_worktrees(project_id)?,
            new_risks,
            resolved_risks,
            events: events.iter().map(|(_, k, _)| k.to_string()).collect(),
        })
    }

    fn evaluate_risks(
        &self,
        project: &Project,
        worktrees: &[Worktree],
        settings: &Settings,
    ) -> Result<(Vec<Risk>, Vec<Risk>)> {
        let store = self.store();
        let mut wfacts = Vec::new();
        for w in worktrees {
            let failures = store.consecutive_auto_failures(&project.id, &w.path)?;
            let skips = match &w.last_snapshot_id {
                Some(sid) => store.skip_counts(sid)?,
                None => vec![],
            };
            wfacts.push(risk::WorktreeFacts {
                worktree: w,
                consecutive_auto_failures: failures,
                latest_skips: skips,
            });
        }
        let facts = risk::ProjectFacts {
            project,
            worktrees: wfacts,
            available_bytes: crate::platform::available_space(&self.root),
            min_free_bytes: settings.min_free_space_bytes,
            referenced_bytes: store.project_referenced_bytes(&project.id)?,
            budget_bytes: settings.repo_budget_bytes,
            stale_minutes: settings.stale_protection_minutes,
            mass_deletion_threshold: settings.mass_deletion_threshold,
            now: crate::now_ms(),
        };
        let findings = risk::evaluate(&facts);
        let mut new = Vec::new();
        let keys: Vec<String> = findings.iter().map(|f| f.dedupe_key.clone()).collect();
        for f in findings {
            if store.raise_risk(&f.risk, &f.dedupe_key)? {
                store.add_event(
                    &project.id,
                    f.risk.worktree_path.as_deref(),
                    ev::RISK_RAISED,
                    None,
                    json!({ "rule": f.risk.rule, "severity": f.risk.severity.as_str(), "riskId": f.risk.id }),
                )?;
                new.push(f.risk);
            }
        }
        let resolved = store.resolve_missing_risks(&project.id, risk::rules::EVALUATED, &keys)?;
        for r in &resolved {
            store.add_event(
                &project.id,
                r.worktree_path.as_deref(),
                ev::RISK_RESOLVED,
                None,
                json!({ "rule": r.rule, "riskId": r.id }),
            )?;
        }
        Ok((new, resolved))
    }

    /// Record a change observed by the file watcher (not yet captured).
    pub fn note_change(&self, project_id: &str, worktree_path: &Path, at: i64) -> Result<()> {
        self.store()
            .note_worktree_change(project_id, &paths::display(worktree_path), at)
    }

    pub fn list_risks(&self, project_id: &str, include_resolved: bool) -> Result<Vec<Risk>> {
        self.store().list_risks(project_id, include_resolved)
    }

    pub fn acknowledge_risk(&self, id: &str) -> Result<()> {
        self.store().acknowledge_risk(id)
    }

    pub fn query_events(&self, q: &EventQuery) -> Result<Vec<TimelineEvent>> {
        self.store().query_events(q)
    }

    // ------------------------------------------------------------------
    // Snapshots
    // ------------------------------------------------------------------

    fn worktree_belongs(&self, project_id: &str, worktree_path: &Path) -> Result<()> {
        let list = self.store().list_worktrees(project_id)?;
        if list
            .iter()
            .any(|w| paths::same_path(Path::new(&w.path), worktree_path))
        {
            Ok(())
        } else {
            Err(CoreError::InvalidTarget(format!(
                "{} is not a worktree of this project",
                paths::display(worktree_path)
            )))
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_snapshot(
        &self,
        project_id: &str,
        worktree_path: &Path,
        trigger: SnapshotTrigger,
        note: Option<String>,
        cancel: &AtomicBool,
        progress: &dyn ProgressSink,
        task_id: &str,
    ) -> Result<CaptureOutcome> {
        self.capture(
            project_id,
            worktree_path,
            trigger,
            note,
            None,
            cancel,
            progress,
            task_id,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn capture(
        &self,
        project_id: &str,
        worktree_path: &Path,
        trigger: SnapshotTrigger,
        note: Option<String>,
        only_paths: Option<Vec<String>>,
        cancel: &AtomicBool,
        progress: &dyn ProgressSink,
        task_id: &str,
    ) -> Result<CaptureOutcome> {
        self.worktree_belongs(project_id, worktree_path)?;
        let key = path_key(&paths::display(worktree_path));
        if !self.busy.lock().unwrap().insert(key.clone()) {
            return Err(CoreError::Busy);
        }
        let _guard = BusyGuard {
            set: &self.busy,
            key,
        };
        let _gate = self.gc_gate.read().unwrap();
        let git = self.git()?;
        let settings = self.settings();
        let policy = CapturePolicy::from_settings(&settings)?;
        let referenced = self.store().project_referenced_bytes(project_id)?;
        let budget_remaining = (settings.repo_budget_bytes - referenced).max(0) as u64;
        let engine = Engine {
            git: &git,
            store: &self.store,
            blobs: &self.blobs,
            manifest_root: &self.manifests,
            policy: &policy,
            cancel,
            progress,
            task_id,
            min_free_bytes: settings.min_free_space_bytes.max(0) as u64,
        };
        let wt_display = paths::display(worktree_path);
        let result = engine.capture(CaptureRequest {
            project_id,
            worktree_path,
            trigger,
            skip_if_unchanged: matches!(
                trigger,
                SnapshotTrigger::Auto | SnapshotTrigger::Checkpoint
            ),
            only_paths,
            budget_remaining: Some(budget_remaining),
            note,
        });
        let store = self.store();
        match &result {
            Ok(CaptureOutcome {
                snapshot: Some(s), ..
            }) => {
                let kind = if s.status == SnapshotStatus::Verified {
                    ev::SNAPSHOT_CREATED
                } else {
                    ev::SNAPSHOT_PARTIAL
                };
                store.add_event(
                    project_id,
                    Some(&wt_display),
                    kind,
                    Some(&s.id),
                    json!({
                        "trigger": s.trigger,
                        "entries": s.entry_count,
                        "captured": s.captured_count,
                        "skipped": s.skipped_count,
                        "failed": s.failed_count,
                        "newBytes": s.new_bytes,
                        "branch": s.branch,
                    }),
                )?;
            }
            Ok(_) => {
                // Unchanged since the last snapshot: protection confirmed now.
                store.confirm_worktree(project_id, &wt_display, crate::now_ms())?;
            }
            Err(e) => {
                if !matches!(e, CoreError::Busy) {
                    store.add_event(
                        project_id,
                        Some(&wt_display),
                        ev::SNAPSHOT_FAILED,
                        None,
                        json!({ "trigger": trigger, "code": e.code(), "message": e.to_string() }),
                    )?;
                }
            }
        }
        result
    }

    pub fn query_snapshots(&self, q: &SnapshotQuery) -> Result<Vec<SnapshotSummary>> {
        self.store().query_snapshots(q)
    }

    pub fn snapshot_detail(&self, id: &str) -> Result<SnapshotDetail> {
        let summary = self.store().get_snapshot(id)?;
        let files = self.store().snapshot_files(id)?;
        let (git, ignored, ok, err) = match summary.manifest_sha256.as_deref() {
            Some(sha) => match manifest::read_manifest(&self.manifests, id, Some(sha)) {
                Ok(m) => (Some(m.git), m.stats.ignored_not_protected, true, None),
                Err(e) => (None, 0, false, Some(e.to_string())),
            },
            None => (None, 0, false, None),
        };
        Ok(SnapshotDetail {
            summary,
            files,
            git,
            ignored_not_protected: ignored,
            manifest_ok: ok,
            manifest_error: err,
        })
    }

    fn load_manifest(&self, id: &str) -> Result<(SnapshotSummary, Manifest)> {
        let summary = self.store().get_snapshot(id)?;
        if !summary.status.is_recoverable() {
            return Err(CoreError::InvalidManifest(format!(
                "snapshot {id} is {}",
                summary.status.as_str()
            )));
        }
        let sha = summary
            .manifest_sha256
            .clone()
            .ok_or_else(|| CoreError::InvalidManifest("no manifest hash".into()))?;
        let m = manifest::read_manifest(&self.manifests, id, Some(&sha))?;
        Ok((summary, m))
    }

    pub fn verify_snapshot(&self, id: &str, deep: bool) -> Result<VerifyResult> {
        let now = crate::now_ms();
        let (summary, m) = match self.load_manifest(id) {
            Ok(v) => v,
            Err(e) => {
                let s = self.store().get_snapshot(id)?;
                self.flag_integrity(&s, &e.to_string())?;
                return Ok(VerifyResult {
                    snapshot_id: id.into(),
                    ok: false,
                    manifest_ok: false,
                    blobs_checked: 0,
                    problems: vec![("manifest".into(), e.to_string())],
                    checked_at: now,
                });
            }
        };
        let refs = m.blob_refs();
        let problems = m.verify_blobs(&self.blobs, deep);
        let ok = problems.is_empty();
        let store = self.store();
        if ok {
            store.mark_verified(id, now)?;
        }
        store.add_event(
            &summary.project_id,
            Some(&summary.worktree_path),
            ev::INTEGRITY_CHECKED,
            Some(id),
            json!({ "ok": ok, "deep": deep, "blobs": refs.len(), "problems": problems.len() }),
        )?;
        drop(store);
        if !ok {
            self.flag_integrity(&summary, &format!("{} blob problem(s)", problems.len()))?;
        }
        Ok(VerifyResult {
            snapshot_id: id.into(),
            ok,
            manifest_ok: true,
            blobs_checked: refs.len(),
            problems,
            checked_at: now,
        })
    }

    fn flag_integrity(&self, s: &SnapshotSummary, detail: &str) -> Result<()> {
        let now = crate::now_ms();
        let r = Risk {
            id: uuid::Uuid::now_v7().to_string(),
            project_id: s.project_id.clone(),
            worktree_path: Some(s.worktree_path.clone()),
            rule: risk::rules::INTEGRITY.into(),
            severity: Severity::Critical,
            confidence: Confidence::High,
            detected_at: now,
            last_seen_at: now,
            resolved_at: None,
            acknowledged: false,
            evidence: json!({ "snapshotId": s.id, "detail": detail }),
        };
        self.store()
            .raise_risk(&r, &format!("{}|{}", risk::rules::INTEGRITY, s.id))?;
        Ok(())
    }

    pub fn set_pinned(&self, id: &str, pinned: bool) -> Result<()> {
        self.store().set_pinned(id, pinned)
    }

    pub fn set_note(&self, id: &str, note: Option<&str>) -> Result<()> {
        self.store().set_note(id, note)
    }

    pub fn file_history(&self, project_id: &str, path: &str) -> Result<Vec<FileVersion>> {
        self.store().file_history(project_id, path, 200)
    }

    /// Content of a vault blob for the diff viewer.
    pub fn blob_content(&self, sha: &str, limit: usize) -> Result<BlobContent> {
        let (data, truncated) = self.blobs.read_prefix(sha, limit)?;
        let size = std::fs::metadata(self.blobs.path_for(sha)?)
            .map(|m| m.len() as i64)
            .unwrap_or(data.len() as i64);
        Ok(to_content(
            Some(sha.to_string()),
            data,
            truncated,
            size,
            "verifiedLocalSnapshot",
        ))
    }

    /// Content of a Git blob (e.g. the HEAD version) for comparison.
    pub fn git_blob_content(
        &self,
        project_id: &str,
        oid: &str,
        limit: usize,
    ) -> Result<BlobContent> {
        let project = self.store().get_project(project_id)?;
        let git = self.git()?;
        let data = forensics::read_git_blob(&git, Path::new(&project.common_dir), oid)?;
        let size = data.len() as i64;
        let truncated = data.len() > limit;
        let data = data[..data.len().min(limit)].to_vec();
        Ok(to_content(None, data, truncated, size, "existingGitObject"))
    }

    /// Current working-tree content (read-only) for comparison.
    pub fn current_file_content(
        &self,
        project_id: &str,
        worktree_path: &Path,
        rel: &str,
        limit: usize,
    ) -> Result<BlobContent> {
        self.worktree_belongs(project_id, worktree_path)?;
        paths::ensure_no_link_ancestors(worktree_path, rel)?;
        let p = paths::safe_join(worktree_path, rel)?;
        let meta = std::fs::symlink_metadata(&p).map_err(|e| CoreError::io(&p, e))?;
        if !meta.is_file() || paths::is_link_like(&meta) {
            return Err(CoreError::UnsafePath(rel.into()));
        }
        let (sha, size) = crate::blob::hash_file(&p)?;
        let f = std::fs::File::open(&p).map_err(|e| CoreError::io(&p, e))?;
        let mut buf = Vec::new();
        use std::io::Read;
        f.take(limit as u64 + 1)
            .read_to_end(&mut buf)
            .map_err(|e| CoreError::io(&p, e))?;
        let truncated = buf.len() > limit;
        buf.truncate(limit);
        Ok(to_content(
            Some(sha),
            buf,
            truncated,
            size as i64,
            "currentWorkingTree",
        ))
    }

    // ------------------------------------------------------------------
    // Recovery
    // ------------------------------------------------------------------

    fn protected_roots(&self) -> Result<Vec<PathBuf>> {
        let mut roots = vec![self.root.clone()];
        let store = self.store();
        for p in store.list_projects()? {
            roots.push(PathBuf::from(&p.common_dir));
            for w in store.list_worktrees(&p.id)? {
                roots.push(PathBuf::from(&w.path));
            }
        }
        Ok(roots)
    }

    pub fn recover_to_directory(
        &self,
        snapshot_id: &str,
        target: &Path,
        selection: Option<Vec<String>>,
        include_staged: bool,
        include_head_files: bool,
        extra_protected: &[PathBuf],
    ) -> Result<RecoveryReport> {
        let (summary, m) = self.load_manifest(snapshot_id)?;
        let mut roots = self.protected_roots()?;
        roots.extend_from_slice(extra_protected);
        let project = self.store().get_project(&summary.project_id)?;
        let git = self.git_opt();
        let head = m.git.head_oid.clone();
        let common = PathBuf::from(&project.common_dir);
        let materializer = |root: &Path, exclude: &[String]| -> Result<Vec<recovery::ItemResult>> {
            let git = git.clone().ok_or(CoreError::GitNotFound)?;
            let head = head
                .clone()
                .ok_or_else(|| CoreError::Other("snapshot has no HEAD commit".into()))?;
            forensics::materialize_tree(&git, &common, &head, root, exclude, "existingGitObject")
        };
        let report = recovery::recover_to_directory(
            &m,
            &summary,
            &self.blobs,
            target,
            &roots,
            selection.as_deref(),
            include_staged,
            if include_head_files {
                Some(&materializer)
            } else {
                None
            },
        )?;
        self.record_recovery(&summary.project_id, Some(snapshot_id), "snapshot", &report)?;
        Ok(report)
    }

    pub fn preview_restore(
        &self,
        snapshot_id: &str,
        selection: Vec<String>,
        variant: Variant,
    ) -> Result<RestorePreview> {
        let (summary, m) = self.load_manifest(snapshot_id)?;
        let wt = PathBuf::from(&summary.worktree_path);
        self.worktree_belongs(&summary.project_id, &wt)?;
        let preview = recovery::build_preview(&m, &wt, &selection, variant)?;
        self.previews
            .lock()
            .unwrap()
            .insert(preview.id.clone(), preview.clone());
        Ok(preview)
    }

    pub fn execute_restore(
        &self,
        preview_id: &str,
        cancel: &AtomicBool,
        progress: &dyn ProgressSink,
    ) -> Result<RecoveryReport> {
        let preview = self
            .previews
            .lock()
            .unwrap()
            .remove(preview_id)
            .ok_or_else(|| {
                CoreError::RecoveryAborted("preview expired; build a new preview".into())
            })?;
        if crate::now_ms() - preview.created_at > 15 * 60_000 {
            return Err(CoreError::RecoveryAborted(
                "preview is older than 15 minutes; build a new preview".into(),
            ));
        }
        let summary = self.store().get_snapshot(&preview.snapshot_id)?;
        let wt = PathBuf::from(&preview.worktree_path);
        let protect = |paths_: &[String]| -> Result<SnapshotSummary> {
            let out = self.capture(
                &summary.project_id,
                &wt,
                SnapshotTrigger::PreRestore,
                Some(format!(
                    "Before restoring from snapshot {}",
                    preview.snapshot_id
                )),
                Some(paths_.to_vec()),
                cancel,
                progress,
                "pre-restore",
            )?;
            out.snapshot.ok_or_else(|| {
                CoreError::RecoveryAborted("protection snapshot was not created".into())
            })
        };
        let report = recovery::execute_in_place(&preview, &self.blobs, &wt, &protect)?;
        self.record_recovery(
            &summary.project_id,
            Some(&preview.snapshot_id),
            "snapshot",
            &report,
        )?;
        Ok(report)
    }

    fn record_recovery(
        &self,
        project_id: &str,
        snapshot_id: Option<&str>,
        source: &str,
        report: &RecoveryReport,
    ) -> Result<()> {
        let store = self.store();
        store.record_recovery(
            &report.id,
            project_id,
            snapshot_id,
            source,
            &report.mode,
            &report.target,
            report.started_at,
            &report.status,
            &serde_json::to_value(report)?,
        )?;
        store.add_event(
            project_id,
            None,
            if matches!(report.status.as_str(), "completed" | "completedWithIssues") {
                ev::RECOVERY_COMPLETED
            } else {
                ev::RECOVERY_FAILED
            },
            snapshot_id,
            json!({
                "reportId": report.id,
                "mode": report.mode,
                "status": report.status,
                "written": report.written,
                "failed": report.failed,
                "target": report.target,
                "provenance": report.provenance,
            }),
        )?;
        Ok(())
    }

    pub fn list_recoveries(&self, project_id: &str) -> Result<Vec<serde_json::Value>> {
        self.store().list_recoveries(project_id)
    }

    // ------------------------------------------------------------------
    // Forensics
    // ------------------------------------------------------------------

    pub fn forensic_scan(
        &self,
        project_id: &str,
        worktree_path: Option<&Path>,
        deep: bool,
        cancel: &AtomicBool,
    ) -> Result<ForensicReport> {
        let project = self.store().get_project(project_id)?;
        let git = self.git()?;
        let cwd = match worktree_path {
            Some(w) if w.is_dir() => {
                self.worktree_belongs(project_id, w)?;
                w.to_path_buf()
            }
            _ => PathBuf::from(&project.common_dir),
        };
        forensics::discover(&git, &cwd, deep, Some(cancel))
    }

    pub fn forensic_commit_files(
        &self,
        project_id: &str,
        oid: &str,
    ) -> Result<Vec<(String, String)>> {
        let project = self.store().get_project(project_id)?;
        forensics::commit_files(&self.git()?, Path::new(&project.common_dir), oid)
    }

    pub fn recover_git_object(
        &self,
        project_id: &str,
        candidate: &ForensicCandidate,
        target: &Path,
        extra_protected: &[PathBuf],
    ) -> Result<RecoveryReport> {
        let project = self.store().get_project(project_id)?;
        let mut roots = self.protected_roots()?;
        roots.extend_from_slice(extra_protected);
        let report = forensics::recover_object(
            &self.git()?,
            Path::new(&project.common_dir),
            candidate,
            target,
            &roots,
        )?;
        self.record_recovery(project_id, None, "gitObject", &report)?;
        Ok(report)
    }

    // ------------------------------------------------------------------
    // Storage management
    // ------------------------------------------------------------------

    pub fn storage_stats(&self) -> Result<StorageStats> {
        let store = self.store();
        let (count, bytes) = store.blob_totals()?;
        let settings = self.settings();
        let mut per = Vec::new();
        for p in store.list_projects()? {
            per.push(ProjectStorage {
                project_id: p.id.clone(),
                name: p.name.clone(),
                snapshot_count: p.snapshot_count,
                referenced_bytes: store.project_referenced_bytes(&p.id)?,
                budget_bytes: settings.repo_budget_bytes,
            });
        }
        let snapshots = store.snapshot_count()?;
        Ok(StorageStats {
            data_dir: paths::display(&self.root),
            blob_count: count,
            blob_bytes: bytes,
            manifest_count: snapshots,
            snapshot_count: snapshots,
            available_bytes: crate::platform::available_space(&self.root),
            per_project: per,
        })
    }

    /// Preview which snapshots the retention policy would prune.
    pub fn retention_plan(&self, project_id: &str) -> Result<Vec<String>> {
        let settings = self.settings();
        let snaps = self.store().snapshots_for_retention(project_id)?;
        Ok(retention::plan(
            &snaps,
            &settings.retention,
            crate::now_ms(),
        ))
    }

    /// Apply retention to every project, then collect unreferenced blobs.
    pub fn run_retention(&self) -> Result<GcReport> {
        let projects = self.list_projects()?;
        let mut all = Vec::new();
        for p in &projects {
            let ids = self.retention_plan(&p.id)?;
            if !ids.is_empty() {
                self.store().add_event(
                    &p.id,
                    None,
                    ev::SNAPSHOT_DELETED,
                    None,
                    json!({ "count": ids.len(), "reason": "retention" }),
                )?;
            }
            all.extend(ids);
        }
        self.delete_snapshots(&all)
    }

    /// Delete specific snapshots (pinned ones are refused) and collect blobs.
    pub fn delete_snapshots(&self, ids: &[String]) -> Result<GcReport> {
        let _gate = self.gc_gate.write().unwrap();
        let mut deletable = Vec::new();
        for id in ids {
            let s = self.store().get_snapshot(id)?;
            if s.pinned {
                return Err(CoreError::Other(format!("snapshot {id} is pinned")));
            }
            if s.status == SnapshotStatus::InProgress {
                continue;
            }
            deletable.push(id.clone());
        }
        // Rows first (in one transaction), then manifests, then blobs that
        // no remaining manifest references.
        self.store.lock().unwrap().delete_snapshots(&deletable)?;
        for id in &deletable {
            let _ = manifest::delete_manifest(&self.manifests, id);
        }
        let mut report = self.collect_blobs()?;
        report.snapshots_pruned = deletable.len();
        Ok(report)
    }

    /// Delete blob files that no snapshot entry references. Must be called
    /// with the gc gate held for writing.
    fn collect_blobs(&self) -> Result<GcReport> {
        let mut report = GcReport::default();
        let unreferenced = self.store().unreferenced_blobs()?;
        for (sha, size) in unreferenced {
            // Double check under the lock right before deleting.
            if self.store().is_blob_referenced(&sha)? {
                continue;
            }
            self.blobs.delete(&sha)?;
            self.store().delete_blob_row(&sha)?;
            report.blobs_deleted += 1;
            report.bytes_freed += size.max(0) as u64;
        }
        // Orphan sweep: files on disk with no database row (left behind by
        // an interrupted capture). Safe because captures are excluded by the gate.
        for sha in self.blobs.list_all() {
            if !self.store().known_blob(&sha)? && !self.store().is_blob_referenced(&sha)? {
                if let Ok(meta) = std::fs::metadata(self.blobs.path_for(&sha)?) {
                    report.bytes_freed += meta.len();
                }
                self.blobs.delete(&sha)?;
                report.orphan_files_deleted += 1;
            }
        }
        report.temp_files_deleted = self.blobs.clean_tmp();
        Ok(report)
    }
}

fn to_content(
    sha: Option<String>,
    data: Vec<u8>,
    truncated: bool,
    size: i64,
    source: &str,
) -> BlobContent {
    let binary = manifest::looks_binary(&data);
    BlobContent {
        sha256: sha,
        size,
        binary,
        truncated,
        text: if binary {
            None
        } else {
            Some(String::from_utf8_lossy(&data).into_owned())
        },
        source: source.into(),
    }
}

// ----------------------------------------------------------------------
// Demo sandbox orchestration
// ----------------------------------------------------------------------

impl Vault {
    /// Build (or rebuild) the sandbox demo inside this vault's root and walk
    /// it through a realistic story: work captured, one worktree deleted,
    /// one moved. Only valid on the demo vault.
    pub fn build_demo(&self) -> Result<Project> {
        if !self.is_demo {
            return Err(CoreError::Other(
                "demo can only be built in the demo vault".into(),
            ));
        }
        // Remove previous demo projects and their data first.
        for p in self.list_projects()? {
            self.remove_project(&p.id, true)?;
        }
        let git = self.git()?;
        let sandbox = self.root.join("sandbox");
        let layout = crate::demo::create(&git, &sandbox)?;
        let project = self.add_project(&layout.main, Some(true))?;
        let cancel = AtomicBool::new(false);
        for wt in [
            &layout.main,
            &layout.agent_cache,
            &layout.agent_metrics,
            &layout.agent_docs,
        ] {
            let canon = PathBuf::from(paths::display(wt));
            self.create_snapshot(
                &project.id,
                &canon,
                SnapshotTrigger::Manual,
                None,
                &cancel,
                &NoProgress,
                "demo",
            )?;
        }
        crate::demo::second_edit(&layout)?;
        self.refresh_project(&project.id)?;
        for wt in [&layout.main, &layout.agent_cache] {
            let canon = PathBuf::from(paths::display(wt));
            self.create_snapshot(
                &project.id,
                &canon,
                SnapshotTrigger::Auto,
                None,
                &cancel,
                &NoProgress,
                "demo",
            )?;
        }
        crate::demo::simulate_loss(&layout)?;
        self.refresh_project(&project.id)?;
        self.store().get_project(&project.id)
    }
}
