//! Independent Snapshot Engine.
//!
//! Captures the dirty state of one working tree – staged content, working
//! tree bytes, untracked files, deletions and renames – into the vault,
//! without touching the repository. A snapshot becomes `verified` only after
//! every blob it references exists and newly written blobs were re-hashed,
//! the manifest was atomically written, and the database transaction
//! committed.

use crate::blob::{BlobStore, StoredBlob};
use crate::error::{CoreError, Result};
use crate::filter::{CapturePolicy, SkipReason};
use crate::git::{self, Git, StatusEntry, StatusKind};
use crate::manifest::*;
use crate::paths;
use crate::store::Store;
use crate::types::*;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::UNIX_EPOCH;

const MAX_INCLUDED_IGNORED_FILES: usize = 20_000;
const STABILITY_RETRIES: usize = 3;
const WORKERS: usize = 4;

pub struct CaptureRequest<'a> {
    pub project_id: &'a str,
    pub worktree_path: &'a Path,
    pub trigger: SnapshotTrigger,
    /// For auto snapshots: skip committing if nothing changed since the last
    /// snapshot of this worktree.
    pub skip_if_unchanged: bool,
    /// Restrict capture to these relative paths (used for pre-restore
    /// protection copies). `None` = full dirty set.
    pub only_paths: Option<Vec<String>>,
    pub budget_remaining: Option<u64>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureOutcome {
    pub snapshot: Option<SnapshotSummary>,
    pub unchanged: bool,
}

pub struct Engine<'a> {
    pub git: &'a Git,
    pub store: &'a Mutex<Store>,
    pub blobs: &'a BlobStore,
    pub manifest_root: &'a Path,
    pub policy: &'a CapturePolicy,
    pub cancel: &'a AtomicBool,
    pub progress: &'a dyn ProgressSink,
    pub task_id: &'a str,
    pub min_free_bytes: u64,
}

/// Work item: one path whose working-tree bytes may need capturing.
#[derive(Debug, Clone)]
struct Item {
    path: String,
    category: String,
    x: char,
    y: char,
    orig_path: Option<String>,
    head_oid: Option<String>,
    head_mode: Option<u32>,
    index_oid: Option<String>,
    index_mode: Option<u32>,
    worktree_mode: u32,
    submodule: bool,
    /// Pre-decided skip (policy) – still recorded in the manifest.
    skip: Option<SkipReason>,
    /// Item may legitimately be absent (tracked file deleted in worktree).
    tracked: bool,
}

fn category_for(e: &StatusEntry) -> String {
    match e.kind {
        StatusKind::Unmerged => "conflicted".into(),
        StatusKind::Untracked => "untracked".into(),
        StatusKind::Ignored => "ignoredIncluded".into(),
        StatusKind::Renamed => {
            if e.x == 'C' {
                "copied".into()
            } else {
                "renamed".into()
            }
        }
        StatusKind::Ordinary => {
            let c = if e.x != '.' { e.x } else { e.y };
            match c {
                'A' => "added".into(),
                'D' => "deleted".into(),
                'T' => "typeChanged".into(),
                _ => "modified".into(),
            }
        }
    }
}

impl<'a> Engine<'a> {
    fn report(&self, phase: &str, done: u64, total: u64, current: Option<String>) {
        self.progress.report(Progress {
            task_id: self.task_id.to_string(),
            phase: phase.to_string(),
            done,
            total,
            current,
        });
    }

    fn check_cancel(&self) -> Result<()> {
        if self.cancel.load(Ordering::Relaxed) {
            Err(CoreError::Cancelled)
        } else {
            Ok(())
        }
    }

    pub fn capture(&self, req: CaptureRequest<'_>) -> Result<CaptureOutcome> {
        let created_at = crate::now_ms();
        let snapshot_id = uuid::Uuid::now_v7().to_string();
        let wt_display = paths::display(req.worktree_path);
        self.store.lock().unwrap().begin_snapshot(
            &snapshot_id,
            req.project_id,
            &wt_display,
            req.trigger,
            created_at,
        )?;
        match self.capture_inner(&snapshot_id, created_at, &wt_display, &req) {
            Ok(outcome) => Ok(outcome),
            Err(e) => {
                let store = self.store.lock().unwrap();
                let _ = store.fail_snapshot(&snapshot_id, e.code(), &e.to_string());
                drop(store);
                let _ = delete_manifest(self.manifest_root, &snapshot_id);
                Err(e)
            }
        }
    }

    fn capture_inner(
        &self,
        snapshot_id: &str,
        created_at: i64,
        wt_display: &str,
        req: &CaptureRequest<'_>,
    ) -> Result<CaptureOutcome> {
        let root = req.worktree_path;
        self.report("scanning", 0, 0, None);
        let ident = git::identify(self.git, root)?;
        self.check_cancel()?;
        let status = git::status(self.git, root, true, Some(self.cancel))?;
        self.check_cancel()?;

        // --- Build work items -------------------------------------------
        let mut items: BTreeMap<String, Item> = BTreeMap::new();
        let mut ignored_not_protected = 0u64;
        let mut ignored_dirs_to_walk: Vec<String> = Vec::new();
        for e in &status.entries {
            match e.kind {
                StatusKind::Ordinary | StatusKind::Renamed | StatusKind::Unmerged => {
                    let index_relevant = e.kind != StatusKind::Unmerged
                        && e.x != '.'
                        && e.x != 'D'
                        && e.oid_index.is_some();
                    items.insert(
                        e.path.clone(),
                        Item {
                            path: e.path.clone(),
                            category: category_for(e),
                            x: e.x,
                            y: e.y,
                            orig_path: e.orig_path.clone(),
                            head_oid: e.oid_head.clone(),
                            head_mode: (e.mode_head != 0).then_some(e.mode_head),
                            index_oid: if index_relevant {
                                e.oid_index.clone()
                            } else {
                                None
                            },
                            index_mode: index_relevant.then_some(e.mode_index),
                            worktree_mode: e.mode_worktree,
                            submodule: e.submodule,
                            skip: if e.submodule {
                                Some(SkipReason::Submodule)
                            } else {
                                None
                            },
                            tracked: true,
                        },
                    );
                }
                StatusKind::Untracked => {
                    if items.contains_key(&e.path) {
                        continue; // e.g. `git rm --cached` – already handled
                    }
                    let skip = if e.is_dir_entry() {
                        // A nested repository shows up as a directory entry.
                        Some(SkipReason::UnsupportedType)
                    } else {
                        self.policy.untracked_decision(&e.path)
                    };
                    items.insert(
                        e.path.clone(),
                        Item {
                            path: e.path.trim_end_matches('/').to_string(),
                            category: "untracked".into(),
                            x: '?',
                            y: '?',
                            orig_path: None,
                            head_oid: None,
                            head_mode: None,
                            index_oid: None,
                            index_mode: None,
                            worktree_mode: 0,
                            submodule: false,
                            skip,
                            tracked: false,
                        },
                    );
                }
                StatusKind::Ignored => {
                    if e.is_dir_entry() && self.policy.walk_ignored_dir(&e.path) {
                        ignored_dirs_to_walk.push(e.path.trim_end_matches('/').to_string());
                    } else if !e.is_dir_entry() && self.policy.ignored_included(&e.path) {
                        items.insert(e.path.clone(), ignored_item(&e.path));
                    } else {
                        ignored_not_protected += 1;
                    }
                }
            }
        }
        for dir in ignored_dirs_to_walk {
            for rel in walk_files(root, &dir, MAX_INCLUDED_IGNORED_FILES) {
                if self.policy.ignored_included(&rel) && !items.contains_key(&rel) {
                    items.insert(rel.clone(), ignored_item(&rel));
                }
            }
        }
        if let Some(only) = &req.only_paths {
            items.retain(|k, _| only.contains(k));
            for p in only {
                if !items.contains_key(p) {
                    // Clean tracked file requested explicitly (pre-restore copy).
                    items.insert(
                        p.clone(),
                        Item {
                            path: p.clone(),
                            category: "unchanged".into(),
                            x: '.',
                            y: '.',
                            orig_path: None,
                            head_oid: None,
                            head_mode: None,
                            index_oid: None,
                            index_mode: None,
                            worktree_mode: 0,
                            submodule: false,
                            skip: None,
                            tracked: true,
                        },
                    );
                }
            }
        }
        // Reject unsafe paths early but keep them visible in the manifest.
        for item in items.values_mut() {
            if paths::validate_rel_path(&item.path).is_err() {
                item.skip = Some(SkipReason::UnsafePath);
            }
        }

        // --- Disk space pre-check ---------------------------------------
        let estimate: u64 = items
            .values()
            .filter(|i| i.skip.is_none())
            .filter_map(|i| {
                paths::safe_join(root, &i.path)
                    .ok()
                    .and_then(|p| std::fs::symlink_metadata(p).ok())
                    .map(|m| m.len().min(self.policy.max_file_bytes))
            })
            .sum();
        if let Some(avail) = crate::platform::available_space(self.blobs.root()) {
            let needed = estimate + self.min_free_bytes.min(512 * 1024 * 1024);
            if avail < needed {
                return Err(CoreError::InsufficientSpace {
                    needed,
                    available: avail,
                });
            }
        }

        // --- Index (staged) content -------------------------------------
        let index_oids: Vec<String> = {
            let mut v: Vec<String> = items
                .values()
                .filter(|i| !i.submodule && i.index_mode != Some(0o160000))
                .filter_map(|i| i.index_oid.clone())
                .collect();
            v.sort();
            v.dedup();
            v
        };
        let mut index_blobs: HashMap<String, StoredBlob> = HashMap::new();
        let new_blobs: Mutex<Vec<(String, u64)>> = Mutex::new(Vec::new());
        if !index_oids.is_empty() {
            self.report("readingIndex", 0, index_oids.len() as u64, None);
            for chunk in index_oids.chunks(200) {
                self.check_cancel()?;
                let objs = self.git.cat_file_batch(root, chunk, Some(self.cancel))?;
                for (oid, obj) in objs {
                    if let Some((kind, bytes)) = obj
                        && kind == "blob"
                    {
                        if bytes.len() as u64 > self.policy.max_file_bytes {
                            continue;
                        }
                        let stored = self.blobs.put_bytes(&bytes)?;
                        if stored.newly_written {
                            new_blobs
                                .lock()
                                .unwrap()
                                .push((stored.sha256.clone(), stored.size));
                        }
                        index_blobs.insert(oid, stored);
                    }
                }
            }
        }

        // --- Working tree content (bounded parallelism) -----------------
        let work: Vec<Item> = items.values().cloned().collect();
        let total = work.len() as u64;
        let done = AtomicU64::new(0);
        let new_bytes = AtomicU64::new(
            new_blobs
                .lock()
                .unwrap()
                .iter()
                .map(|(_, s)| *s)
                .sum::<u64>(),
        );
        let budget = req.budget_remaining;
        let queue = Mutex::new(work.into_iter().enumerate());
        let results: Mutex<Vec<(usize, ManifestEntry)>> = Mutex::new(Vec::new());
        let first_error: Mutex<Option<CoreError>> = Mutex::new(None);
        self.report("capturing", 0, total, None);
        std::thread::scope(|s| {
            for _ in 0..WORKERS {
                s.spawn(|| {
                    loop {
                        if self.cancel.load(Ordering::Relaxed)
                            || first_error.lock().unwrap().is_some()
                        {
                            return;
                        }
                        let next = queue.lock().unwrap().next();
                        let Some((idx, item)) = next else { return };
                        match self.capture_item(
                            root,
                            &item,
                            &index_blobs,
                            &new_bytes,
                            budget,
                            &new_blobs,
                        ) {
                            Ok(entry) => results.lock().unwrap().push((idx, entry)),
                            Err(e) => {
                                *first_error.lock().unwrap() = Some(e);
                                return;
                            }
                        }
                        let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                        if d.is_multiple_of(16) || d == total {
                            self.report("capturing", d, total, Some(item.path.clone()));
                        }
                    }
                });
            }
        });
        if let Some(e) = first_error.into_inner().unwrap() {
            return Err(e);
        }
        self.check_cancel()?;
        let mut results = results.into_inner().unwrap();
        results.sort_by_key(|(i, _)| *i);
        let entries: Vec<ManifestEntry> = results.into_iter().map(|(_, e)| e).collect();

        // --- Change detection for auto snapshots ------------------------
        let content_key = content_key(&entries, &ident);
        if req.skip_if_unchanged && entries.is_empty() {
            // Clean worktree: Git history already holds this state.
            self.store
                .lock()
                .unwrap()
                .discard_snapshot_row(snapshot_id)?;
            return Ok(CaptureOutcome {
                snapshot: None,
                unchanged: true,
            });
        }
        if req.skip_if_unchanged {
            let last = self
                .store
                .lock()
                .unwrap()
                .last_content_key(req.project_id, wt_display)?;
            if last.as_deref() == Some(content_key.as_str()) {
                self.store
                    .lock()
                    .unwrap()
                    .discard_snapshot_row(snapshot_id)?;
                return Ok(CaptureOutcome {
                    snapshot: None,
                    unchanged: true,
                });
            }
        }

        // --- Stats & manifest -------------------------------------------
        let mut stats = ManifestStats {
            entries: entries.len() as u64,
            ignored_not_protected,
            ..Default::default()
        };
        for e in &entries {
            match e.worktree.state {
                WtState::Present | WtState::Absent => stats.captured += 1,
                WtState::Skipped => stats.skipped += 1,
                WtState::Failed => stats.failed += 1,
            }
            stats.protected_bytes += e
                .worktree
                .size
                .filter(|_| e.worktree.blob.is_some())
                .unwrap_or(0);
            if let Some(i) = &e.index
                && i.blob.is_some()
                && i.blob != e.worktree.blob
            {
                stats.protected_bytes += i.size.unwrap_or(0);
            }
        }
        let new_list = new_blobs.into_inner().unwrap();
        stats.new_bytes = new_list.iter().map(|(_, s)| *s).sum();
        let budget_skips = entries.iter().any(|e| {
            e.worktree.skip_reason.as_deref() == Some(SkipReason::BudgetExceeded.as_str())
        });

        let manifest = Manifest {
            format_version: FORMAT_VERSION,
            snapshot_id: snapshot_id.to_string(),
            project_id: req.project_id.to_string(),
            created_at,
            finished_at: crate::now_ms(),
            trigger: req.trigger.as_str().to_string(),
            worktree_path: wt_display.to_string(),
            git: GitContext {
                branch: ident.branch.clone(),
                head_oid: ident.head_oid.clone(),
                detached: ident.detached,
                unborn: ident.unborn,
                upstream: status.upstream.clone(),
                git_dir: ident.git_dir.clone(),
                common_dir: ident.common_dir.clone(),
                sparse_checkout: ident.sparse_checkout,
            },
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            policy: serde_json::json!({
                "maxFileBytes": self.policy.max_file_bytes,
                "includeUntracked": self.policy.include_untracked,
                "includeSensitive": self.policy.include_sensitive,
                "note": req.note,
            }),
            entries,
            stats,
        };

        // --- Verify before committing -----------------------------------
        self.report("verifying", 0, manifest.blob_refs().len() as u64, None);
        let newly: std::collections::HashSet<&String> = new_list.iter().map(|(s, _)| s).collect();
        for b in manifest.blob_refs() {
            self.check_cancel()?;
            if newly.contains(&b) {
                self.blobs.verify(&b)?;
            } else if !self.blobs.contains(&b) {
                return Err(CoreError::Integrity {
                    what: format!("blob {b}"),
                    expected: "present".into(),
                    actual: "missing".into(),
                });
            }
        }
        let manifest_sha = write_manifest(self.manifest_root, &manifest)?;
        // Read back to prove the manifest is durable and intact.
        read_manifest(self.manifest_root, snapshot_id, Some(&manifest_sha))?;

        let status = if manifest.stats.failed > 0 || budget_skips {
            SnapshotStatus::Partial
        } else {
            SnapshotStatus::Verified
        };
        self.report("committing", 0, 0, None);
        {
            let mut store = self.store.lock().unwrap();
            store.commit_snapshot(&manifest, &manifest_sha, status, &content_key, &new_list)?;
            if let Some(n) = &req.note {
                store.set_note(snapshot_id, Some(n))?;
            }
        }
        let summary = self.store.lock().unwrap().get_snapshot(snapshot_id)?;
        Ok(CaptureOutcome {
            snapshot: Some(summary),
            unchanged: false,
        })
    }

    fn capture_item(
        &self,
        root: &Path,
        item: &Item,
        index_blobs: &HashMap<String, StoredBlob>,
        new_bytes: &AtomicU64,
        budget: Option<u64>,
        new_blobs: &Mutex<Vec<(String, u64)>>,
    ) -> Result<ManifestEntry> {
        let index = item.index_oid.as_ref().map(|oid| {
            let stored = index_blobs.get(oid);
            IndexState {
                git_oid: oid.clone(),
                mode: item.index_mode.unwrap_or(0),
                blob: stored.map(|s| s.sha256.clone()),
                size: stored.map(|s| s.size),
            }
        });
        let mut wt = WorktreeState {
            state: WtState::Skipped,
            kind: if item.submodule {
                EntryKind::Submodule
            } else {
                EntryKind::File
            },
            blob: None,
            size: None,
            executable: item.worktree_mode == 0o100755,
            mtime_ms: None,
            link_target: None,
            link_escapes: false,
            skip_reason: None,
            lfs_pointer: false,
            binary: false,
        };
        let entry = |wt: WorktreeState, index: Option<IndexState>| ManifestEntry {
            path: item.path.clone(),
            orig_path: item.orig_path.clone(),
            category: item.category.clone(),
            x: item.x.to_string(),
            y: item.y.to_string(),
            head_oid: item.head_oid.clone(),
            head_mode: item.head_mode,
            index,
            worktree: wt,
        };

        if let Some(skip) = item.skip {
            if skip == SkipReason::UnsupportedType {
                wt.kind = EntryKind::Other;
            }
            wt.skip_reason = Some(skip.as_str().into());
            return Ok(entry(wt, index));
        }
        // Existing ancestors must not be links: never read through a junction.
        if paths::ensure_no_link_ancestors(root, &item.path).is_err() {
            wt.skip_reason = Some(SkipReason::UnsafePath.as_str().into());
            return Ok(entry(wt, index));
        }
        let full = paths::safe_join(root, &item.path)?;
        let meta = match std::fs::symlink_metadata(&full) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                wt.state = WtState::Absent;
                if !item.tracked {
                    // Untracked file vanished between status and capture.
                    wt.state = WtState::Failed;
                    wt.skip_reason = Some(SkipReason::Unstable.as_str().into());
                }
                return Ok(entry(wt, index));
            }
            Err(_) => {
                wt.state = WtState::Failed;
                wt.skip_reason = Some(SkipReason::Unreadable.as_str().into());
                return Ok(entry(wt, index));
            }
        };
        wt.mtime_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64);

        if paths::is_link_like(&meta) {
            wt.kind = EntryKind::Symlink;
            match std::fs::read_link(&full) {
                Ok(target) => {
                    let t = target.to_string_lossy().replace('\\', "/");
                    let resolved = if target.is_absolute() {
                        target.clone()
                    } else {
                        full.parent().unwrap_or(root).join(&target)
                    };
                    wt.link_escapes = !paths::is_within(&paths::lexical_normalize(&resolved), root);
                    // Store the link text itself so it can be recreated or reviewed.
                    let stored = self.blobs.put_bytes(t.as_bytes())?;
                    if stored.newly_written {
                        new_blobs
                            .lock()
                            .unwrap()
                            .push((stored.sha256.clone(), stored.size));
                    }
                    wt.size = Some(stored.size);
                    wt.blob = Some(stored.sha256);
                    wt.link_target = Some(t);
                    wt.state = WtState::Present;
                }
                Err(_) => {
                    wt.state = WtState::Failed;
                    wt.skip_reason = Some(SkipReason::Unreadable.as_str().into());
                }
            }
            return Ok(entry(wt, index));
        }
        if !meta.is_file() {
            wt.kind = EntryKind::Other;
            wt.skip_reason = Some(SkipReason::UnsupportedType.as_str().into());
            return Ok(entry(wt, index));
        }
        let size = meta.len();
        wt.size = Some(size);
        if let Some(reason) = self.policy.size_decision(size) {
            wt.skip_reason = Some(reason.as_str().into());
            return Ok(entry(wt, index));
        }
        if let Some(limit) = budget
            && new_bytes.load(Ordering::Relaxed) + size > limit
        {
            wt.skip_reason = Some(SkipReason::BudgetExceeded.as_str().into());
            return Ok(entry(wt, index));
        }

        // Read with a stability check: metadata before and after must agree.
        for attempt in 0..STABILITY_RETRIES {
            let before = file_fingerprint(&full);
            let stored = match self.blobs.put_file(&full) {
                Ok(s) => s,
                Err(CoreError::Io { source, .. })
                    if source.kind() == std::io::ErrorKind::NotFound =>
                {
                    wt.state = if item.tracked {
                        WtState::Absent
                    } else {
                        WtState::Failed
                    };
                    if !item.tracked {
                        wt.skip_reason = Some(SkipReason::Unstable.as_str().into());
                    }
                    wt.size = None;
                    return Ok(entry(wt, index));
                }
                Err(CoreError::Io { path, source }) if path == full => {
                    let _ = source;
                    wt.state = WtState::Failed;
                    wt.skip_reason = Some(SkipReason::Unreadable.as_str().into());
                    return Ok(entry(wt, index));
                }
                Err(e) => return Err(e),
            };
            let after = file_fingerprint(&full);
            if before.is_some() && before == after && Some(stored.size) == before.map(|f| f.0) {
                if stored.newly_written {
                    new_bytes.fetch_add(stored.size, Ordering::Relaxed);
                    new_blobs
                        .lock()
                        .unwrap()
                        .push((stored.sha256.clone(), stored.size));
                }
                let (prefix, _) = self.blobs.read_prefix(&stored.sha256, 8192)?;
                wt.binary = looks_binary(&prefix);
                wt.lfs_pointer = is_lfs_pointer(&prefix);
                wt.state = WtState::Present;
                wt.size = Some(stored.size);
                wt.blob = Some(stored.sha256);
                return Ok(entry(wt, index));
            }
            // Changed while reading. The blob just written is unreferenced; it
            // is not deleted here (another worker may be deduplicating against
            // it) and is reclaimed later by the orphan sweep in retention GC.
            if attempt + 1 < STABILITY_RETRIES {
                std::thread::sleep(std::time::Duration::from_millis(150));
            }
        }
        wt.state = WtState::Failed;
        wt.skip_reason = Some(SkipReason::Unstable.as_str().into());
        Ok(entry(wt, index))
    }
}

fn ignored_item(path: &str) -> Item {
    Item {
        path: path.to_string(),
        category: "ignoredIncluded".into(),
        x: '!',
        y: '!',
        orig_path: None,
        head_oid: None,
        head_mode: None,
        index_oid: None,
        index_mode: None,
        worktree_mode: 0,
        submodule: false,
        skip: None,
        tracked: false,
    }
}

/// (size, mtime-nanos) used to detect concurrent modification.
fn file_fingerprint(p: &Path) -> Option<(u64, u128)> {
    let m = std::fs::symlink_metadata(p).ok()?;
    let t = m
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some((m.len(), t))
}

/// Recursively list files below `root/rel_dir` without following links.
fn walk_files(root: &Path, rel_dir: &str, cap: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack: Vec<String> = vec![rel_dir.to_string()];
    while let Some(rel) = stack.pop() {
        let Ok(dir) = paths::safe_join(root, &rel) else {
            continue;
        };
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for ent in rd.flatten() {
            let Some(name) = ent.file_name().to_str().map(str::to_string) else {
                continue;
            };
            let child = format!("{rel}/{name}");
            let Ok(meta) = std::fs::symlink_metadata(ent.path()) else {
                continue;
            };
            if paths::is_link_like(&meta) || meta.is_file() {
                out.push(child);
            } else if meta.is_dir() && name != ".git" {
                stack.push(child);
            }
            if out.len() >= cap {
                return out;
            }
        }
    }
    out
}

/// Stable digest of the captured state, used to avoid redundant snapshots.
fn content_key(entries: &[ManifestEntry], ident: &git::RepoIdentity) -> String {
    let mut h = Sha256::new();
    h.update(ident.head_oid.as_deref().unwrap_or("-"));
    h.update(ident.branch.as_deref().unwrap_or("-"));
    for e in entries {
        h.update(e.path.as_bytes());
        h.update([0]);
        h.update(e.worktree.state.as_str());
        h.update(e.worktree.blob.as_deref().unwrap_or("-"));
        h.update(e.index.as_ref().map(|i| i.git_oid.as_str()).unwrap_or("-"));
        h.update(e.worktree.skip_reason.as_deref().unwrap_or("-"));
        h.update([1]);
    }
    hex::encode(h.finalize())
}

/// Path of a captured file inside the worktree (for callers).
pub fn worktree_file(root: &Path, rel: &str) -> Result<PathBuf> {
    paths::safe_join(root, rel)
}
