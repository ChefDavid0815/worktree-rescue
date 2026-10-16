//! Safe Recovery Workspace.
//!
//! Six phases: source verification, target selection, conflict detection,
//! change preview, execution, post-recovery validation. The default mode
//! writes into a *new, separate directory* and never touches the working
//! tree. In-place restore of selected files is available only through a
//! preview that fingerprints every target; execution aborts if anything
//! changed since the preview, takes a verified protection snapshot of the
//! files about to be replaced, writes via temp file + rename and re-hashes
//! the result. The Git index is never modified.

use crate::blob::{BlobStore, hash_file};
use crate::error::{CoreError, Result};
use crate::manifest::{EntryKind, Manifest, ManifestEntry, WtState};
use crate::paths;
use crate::types::SnapshotSummary;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const MAX_IN_PLACE_FILES: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Variant {
    /// Working-tree bytes as they were on disk.
    WorkingTree,
    /// Staged (index) content.
    Staged,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemResult {
    pub path: String,
    /// written | skipped | failed | listed
    pub outcome: String,
    pub detail: Option<String>,
    pub variant: Option<String>,
    pub expected_sha256: Option<String>,
    pub written_sha256: Option<String>,
    pub bytes: Option<u64>,
    pub destination: Option<String>,
    pub provenance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhaseRecord {
    pub phase: String,
    pub ok: bool,
    pub detail: String,
    pub at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryReport {
    pub id: String,
    pub mode: String,
    pub source: String,
    pub provenance: String,
    pub snapshot_id: Option<String>,
    pub target: String,
    pub started_at: i64,
    pub finished_at: i64,
    /// completed | completedWithIssues | aborted | failed
    pub status: String,
    pub phases: Vec<PhaseRecord>,
    pub items: Vec<ItemResult>,
    pub written: u64,
    pub skipped: u64,
    pub failed: u64,
    pub bytes_written: u64,
    pub protection_snapshot_id: Option<String>,
    pub git_context: serde_json::Value,
    pub notes: Vec<String>,
}

impl RecoveryReport {
    pub fn new(
        mode: &str,
        source: &str,
        provenance: &str,
        snapshot_id: Option<String>,
        target: &str,
    ) -> Self {
        RecoveryReport {
            id: uuid::Uuid::now_v7().to_string(),
            mode: mode.into(),
            source: source.into(),
            provenance: provenance.into(),
            snapshot_id,
            target: target.into(),
            started_at: crate::now_ms(),
            finished_at: 0,
            status: "failed".into(),
            phases: vec![],
            items: vec![],
            written: 0,
            skipped: 0,
            failed: 0,
            bytes_written: 0,
            protection_snapshot_id: None,
            git_context: serde_json::Value::Null,
            notes: vec![],
        }
    }

    pub fn phase(&mut self, phase: &str, ok: bool, detail: impl Into<String>) {
        self.phases.push(PhaseRecord {
            phase: phase.into(),
            ok,
            detail: detail.into(),
            at: crate::now_ms(),
        });
    }

    pub fn push(&mut self, item: ItemResult) {
        match item.outcome.as_str() {
            "written" => {
                self.written += 1;
                self.bytes_written += item.bytes.unwrap_or(0);
            }
            "failed" => self.failed += 1,
            _ => self.skipped += 1,
        }
        self.items.push(item);
    }

    pub fn finish(&mut self) {
        self.finished_at = crate::now_ms();
        if self.status != "aborted" {
            self.status = if self.failed > 0 {
                if self.written > 0 {
                    "completedWithIssues".into()
                } else {
                    "failed".into()
                }
            } else {
                "completed".into()
            };
        }
    }
}

/// Validate a separate-directory target. It must be absolute, must not be
/// inside any protected root (worktrees, vault), and must be absent or an
/// empty directory.
pub fn check_separate_target(target: &Path, protected_roots: &[PathBuf]) -> Result<()> {
    if !target.is_absolute() {
        return Err(CoreError::InvalidTarget(
            "target must be an absolute path".into(),
        ));
    }
    let normalized = paths::lexical_normalize(target);
    for root in protected_roots {
        if paths::is_within(&normalized, root) || paths::is_within(root, &normalized) {
            return Err(CoreError::InvalidTarget(format!(
                "{} overlaps a protected location ({})",
                paths::display(&normalized),
                paths::display(root)
            )));
        }
    }
    match fs::symlink_metadata(&normalized) {
        Ok(meta) => {
            if paths::is_link_like(&meta) || !meta.is_dir() {
                return Err(CoreError::InvalidTarget(
                    "target exists and is not a plain directory".into(),
                ));
            }
            let mut rd = fs::read_dir(&normalized).map_err(|e| CoreError::io(&normalized, e))?;
            if rd.next().is_some() {
                return Err(CoreError::InvalidTarget(
                    "target directory is not empty".into(),
                ));
            }
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(CoreError::io(&normalized, e)),
    }
}

fn select<'m>(m: &'m Manifest, paths_sel: Option<&[String]>) -> Vec<&'m ManifestEntry> {
    match paths_sel {
        Some(sel) if !sel.is_empty() => {
            m.entries.iter().filter(|e| sel.contains(&e.path)).collect()
        }
        _ => m.entries.iter().collect(),
    }
}

/// Phase 1: re-hash every blob the selection depends on.
pub fn verify_source(m: &Manifest, blobs: &BlobStore, sel: Option<&[String]>) -> Result<usize> {
    let mut n = 0;
    for e in select(m, sel) {
        for b in [
            e.worktree.blob.as_ref(),
            e.index.as_ref().and_then(|i| i.blob.as_ref()),
        ]
        .into_iter()
        .flatten()
        {
            blobs.verify(b)?;
            n += 1;
        }
    }
    Ok(n)
}

/// Supplies unchanged files from the snapshot's HEAD commit.
pub type HeadMaterializer<'a> = &'a dyn Fn(&Path, &[String]) -> Result<Vec<ItemResult>>;

/// Recover a snapshot (or a selection of it) into a new directory.
#[allow(clippy::too_many_arguments)]
pub fn recover_to_directory(
    m: &Manifest,
    summary: &SnapshotSummary,
    blobs: &BlobStore,
    target: &Path,
    protected_roots: &[PathBuf],
    sel: Option<&[String]>,
    include_staged: bool,
    head_materializer: Option<HeadMaterializer<'_>>,
) -> Result<RecoveryReport> {
    let mut report = RecoveryReport::new(
        "separateDirectory",
        "snapshot",
        "verifiedLocalSnapshot",
        Some(m.snapshot_id.clone()),
        &paths::display(target),
    );
    report.git_context = serde_json::to_value(&m.git)?;

    // 1. Source verification
    match verify_source(m, blobs, sel) {
        Ok(n) => report.phase("sourceVerification", true, format!("{n} blobs re-hashed")),
        Err(e) => {
            report.phase("sourceVerification", false, e.to_string());
            report.status = "aborted".into();
            report.finish();
            return Ok(report);
        }
    }
    // 2. Target selection
    if let Err(e) = check_separate_target(target, protected_roots) {
        report.phase("targetSelection", false, e.to_string());
        report.status = "aborted".into();
        report.finish();
        return Ok(report);
    }
    report.phase("targetSelection", true, paths::display(target));
    // 3. Conflict detection – a fresh empty directory cannot conflict, but
    // paths that differ only by case would collide on Windows.
    let entries = select(m, sel);
    let mut seen = std::collections::HashMap::new();
    let mut collisions = Vec::new();
    for e in &entries {
        let k = if cfg!(windows) {
            e.path.to_lowercase()
        } else {
            e.path.clone()
        };
        if let Some(prev) = seen.insert(k, e.path.clone()) {
            collisions.push(format!("{prev} / {}", e.path));
        }
    }
    report.phase(
        "conflictDetection",
        true,
        if collisions.is_empty() {
            "no conflicts".to_string()
        } else {
            format!("case collisions: {}", collisions.join(", "))
        },
    );
    report.phase(
        "changePreview",
        true,
        format!("{} entries selected", entries.len()),
    );

    // 4/5. Execution
    let wt_root = target.join("working-tree");
    let staged_root = target.join("staged");
    fs::create_dir_all(&wt_root).map_err(|e| CoreError::io(&wt_root, e))?;
    let mut symlinks = Vec::new();
    for e in &entries {
        let base = ItemResult {
            path: e.path.clone(),
            outcome: "skipped".into(),
            detail: None,
            variant: Some("workingTree".into()),
            expected_sha256: e.worktree.blob.clone(),
            written_sha256: None,
            bytes: None,
            destination: None,
            provenance: "verifiedLocalSnapshot".into(),
        };
        match (e.worktree.state, e.worktree.kind) {
            (WtState::Present, EntryKind::File) => {
                let sha = e.worktree.blob.clone().expect("validated manifest");
                report.push(write_one(blobs, &sha, &wt_root, &e.path, base));
            }
            (WtState::Present, EntryKind::Symlink) => {
                symlinks.push(format!(
                    "{} -> {}{}",
                    e.path,
                    e.worktree.link_target.clone().unwrap_or_default(),
                    if e.worktree.link_escapes {
                        "  (points outside the worktree)"
                    } else {
                        ""
                    }
                ));
                report.push(ItemResult {
                    outcome: "listed".into(),
                    detail: Some("symlinkListed".into()),
                    ..base
                });
            }
            (WtState::Absent, _) => report.push(ItemResult {
                outcome: "listed".into(),
                detail: Some("deletedInSnapshot".into()),
                expected_sha256: None,
                ..base
            }),
            _ => report.push(ItemResult {
                outcome: "skipped".into(),
                detail: Some(
                    e.worktree
                        .skip_reason
                        .clone()
                        .unwrap_or_else(|| "notCaptured".into()),
                ),
                expected_sha256: None,
                ..base
            }),
        }
        if include_staged
            && let Some(idx) = &e.index
            && let Some(sha) = &idx.blob
            && (Some(sha) != e.worktree.blob.as_ref() || e.worktree.state != WtState::Present)
        {
            let item = ItemResult {
                path: e.path.clone(),
                outcome: "skipped".into(),
                detail: None,
                variant: Some("staged".into()),
                expected_sha256: Some(sha.clone()),
                written_sha256: None,
                bytes: None,
                destination: None,
                provenance: "verifiedLocalSnapshot".into(),
            };
            report.push(write_one(blobs, sha, &staged_root, &e.path, item));
        }
    }
    if !symlinks.is_empty() {
        let p = target.join("symlinks.txt");
        let _ = fs::write(&p, symlinks.join("\n") + "\n");
    }
    if let Some(materialize) = head_materializer {
        let captured: Vec<String> = m.entries.iter().map(|e| e.path.clone()).collect();
        match materialize(&wt_root, &captured) {
            Ok(items) => {
                let n = items.len();
                for it in items {
                    report.push(it);
                }
                report.notes.push(format!("headFilesMaterialized:{n}"));
            }
            Err(e) => report.notes.push(format!("headMaterializeFailed:{e}")),
        }
    }
    report.phase(
        "recoveryExecution",
        report.failed == 0,
        format!("{} written, {} failed", report.written, report.failed),
    );

    // 6. Post-recovery validation: re-hash every written file.
    let mut bad = 0;
    for it in report.items.iter_mut().filter(|i| i.outcome == "written") {
        if let (Some(dest), Some(exp)) = (&it.destination, &it.expected_sha256) {
            match hash_file(Path::new(dest)) {
                Ok((h, _)) if h.eq_ignore_ascii_case(exp) => it.written_sha256 = Some(h),
                Ok((h, _)) => {
                    it.written_sha256 = Some(h);
                    it.outcome = "failed".into();
                    it.detail = Some("hashMismatchAfterWrite".into());
                    bad += 1;
                }
                Err(e) => {
                    it.outcome = "failed".into();
                    it.detail = Some(e.to_string());
                    bad += 1;
                }
            }
        }
    }
    if bad > 0 {
        report.failed += bad;
        report.written -= bad;
    }
    report.phase(
        "postRecoveryValidation",
        bad == 0,
        format!("{bad} mismatches"),
    );
    report.notes.push(format!(
        "snapshotCreatedAt:{} branch:{} head:{}",
        summary.created_at,
        m.git.branch.clone().unwrap_or_default(),
        m.git.head_oid.clone().unwrap_or_default()
    ));
    report.finish();
    write_report_files(target, &report)?;
    Ok(report)
}

fn write_one(
    blobs: &BlobStore,
    sha: &str,
    root: &Path,
    rel: &str,
    mut item: ItemResult,
) -> ItemResult {
    let dest = match paths::safe_join(root, rel) {
        Ok(d) => d,
        Err(e) => {
            item.outcome = "failed".into();
            item.detail = Some(e.to_string());
            return item;
        }
    };
    if let Err(e) = paths::ensure_no_link_ancestors(root, rel) {
        item.outcome = "failed".into();
        item.detail = Some(e.to_string());
        return item;
    }
    match blobs.export_to(sha, &dest) {
        Ok(n) => {
            item.outcome = "written".into();
            item.bytes = Some(n);
            item.destination = Some(paths::display(&dest));
        }
        Err(e) => {
            item.outcome = "failed".into();
            item.detail = Some(e.to_string());
        }
    }
    item
}

pub fn write_report_files(target: &Path, report: &RecoveryReport) -> Result<()> {
    fs::create_dir_all(target).map_err(|e| CoreError::io(target, e))?;
    let json_path = target.join("recovery-report.json");
    fs::write(&json_path, serde_json::to_vec_pretty(report)?)
        .map_err(|e| CoreError::io(&json_path, e))?;
    let md_path = target.join("RECOVERY-README.md");
    let mut md = String::new();
    md.push_str("# Worktree Rescue recovery\n\n");
    md.push_str(&format!("- Report ID: `{}`\n", report.id));
    md.push_str(&format!(
        "- Source: {} ({})\n",
        report.source, report.provenance
    ));
    if let Some(s) = &report.snapshot_id {
        md.push_str(&format!("- Snapshot: `{s}`\n"));
    }
    md.push_str(&format!("- Status: **{}**\n", report.status));
    md.push_str(&format!(
        "- Files written: {} · skipped/listed: {} · failed: {}\n\n",
        report.written, report.skipped, report.failed
    ));
    md.push_str("`working-tree/` holds file contents as they were on disk. ");
    md.push_str(
        "`staged/` (if present) holds staged versions that differed from the working tree. ",
    );
    md.push_str(
        "Every written file was re-hashed and compared against the snapshot's SHA-256.\n\n",
    );
    md.push_str("The Git index was not reconstructed. Inspect `recovery-report.json` for the original staged object ids.\n\n");
    md.push_str("## Phases\n\n");
    for p in &report.phases {
        md.push_str(&format!(
            "- {} — {} — {}\n",
            p.phase,
            if p.ok { "ok" } else { "FAILED" },
            p.detail
        ));
    }
    let problems: Vec<&ItemResult> = report
        .items
        .iter()
        .filter(|i| i.outcome == "failed")
        .collect();
    if !problems.is_empty() {
        md.push_str("\n## Failed items\n\n");
        for i in problems {
            md.push_str(&format!(
                "- `{}`: {}\n",
                i.path,
                i.detail.clone().unwrap_or_default()
            ));
        }
    }
    fs::write(&md_path, md).map_err(|e| CoreError::io(&md_path, e))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// In-place restore of selected files
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewItem {
    pub path: String,
    /// create | overwrite | identical | blocked
    pub action: String,
    pub blocked_reason: Option<String>,
    pub current_sha256: Option<String>,
    pub current_size: Option<u64>,
    pub snapshot_sha256: Option<String>,
    pub snapshot_size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestorePreview {
    pub id: String,
    pub snapshot_id: String,
    pub worktree_path: String,
    pub variant: Variant,
    pub created_at: i64,
    pub items: Vec<PreviewItem>,
    pub overwrite_count: usize,
    pub create_count: usize,
    pub blocked_count: usize,
}

/// Exact-case existence check: on case-insensitive file systems a file
/// `Readme.md` must not be silently replaced when restoring `README.md`.
fn exact_case_exists(path: &Path) -> std::io::Result<Option<bool>> {
    let Some(parent) = path.parent() else {
        return Ok(None);
    };
    let Some(name) = path.file_name() else {
        return Ok(None);
    };
    let rd = match fs::read_dir(parent) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let lower = name.to_string_lossy().to_lowercase();
    let mut ci_match = false;
    for ent in rd.flatten() {
        let n = ent.file_name();
        if n == name {
            return Ok(Some(true));
        }
        if n.to_string_lossy().to_lowercase() == lower {
            ci_match = true;
        }
    }
    Ok(if ci_match { Some(false) } else { None })
}

pub fn build_preview(
    m: &Manifest,
    worktree: &Path,
    sel: &[String],
    variant: Variant,
) -> Result<RestorePreview> {
    if sel.is_empty() {
        return Err(CoreError::InvalidTarget("no files selected".into()));
    }
    if sel.len() > MAX_IN_PLACE_FILES {
        return Err(CoreError::InvalidTarget(format!(
            "in-place restore is limited to {MAX_IN_PLACE_FILES} files; recover to a separate directory instead"
        )));
    }
    if !worktree.is_dir() {
        return Err(CoreError::InvalidTarget("worktree is not reachable".into()));
    }
    let mut items = Vec::new();
    for rel in sel {
        let Some(e) = m.entries.iter().find(|e| &e.path == rel) else {
            return Err(CoreError::InvalidTarget(format!(
                "{rel} is not in this snapshot"
            )));
        };
        let (snap_sha, snap_size) = match variant {
            Variant::WorkingTree
                if e.worktree.state == WtState::Present && e.worktree.kind == EntryKind::File =>
            {
                (e.worktree.blob.clone(), e.worktree.size)
            }
            Variant::Staged => match &e.index {
                Some(i) if i.blob.is_some() => (i.blob.clone(), i.size),
                _ => (None, None),
            },
            _ => (None, None),
        };
        let mut item = PreviewItem {
            path: rel.clone(),
            action: "blocked".into(),
            blocked_reason: None,
            current_sha256: None,
            current_size: None,
            snapshot_sha256: snap_sha.clone(),
            snapshot_size: snap_size,
        };
        if snap_sha.is_none() {
            item.blocked_reason = Some(match (variant, e.worktree.state, e.worktree.kind) {
                (Variant::WorkingTree, WtState::Absent, _) => "deletionNotReplayed".into(),
                (_, _, EntryKind::Symlink) => "symlinkNotRestoredInPlace".into(),
                (Variant::Staged, _, _) => "noStagedContent".into(),
                _ => "contentNotCaptured".into(),
            });
            items.push(item);
            continue;
        }
        if let Err(err) = paths::ensure_no_link_ancestors(worktree, rel) {
            item.blocked_reason = Some(format!("unsafePath: {err}"));
            items.push(item);
            continue;
        }
        let dest = paths::safe_join(worktree, rel)?;
        match fs::symlink_metadata(&dest) {
            Ok(meta) => {
                if paths::is_link_like(&meta) || !meta.is_file() {
                    item.blocked_reason = Some("targetNotRegularFile".into());
                } else {
                    match exact_case_exists(&dest) {
                        Ok(Some(true)) => {
                            let (h, n) = hash_file(&dest)?;
                            item.action = if Some(&h) == snap_sha.as_ref() {
                                "identical".into()
                            } else {
                                "overwrite".into()
                            };
                            item.current_sha256 = Some(h);
                            item.current_size = Some(n);
                        }
                        Ok(_) => item.blocked_reason = Some("caseMismatch".into()),
                        Err(e) => item.blocked_reason = Some(format!("unreadable: {e}")),
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => match exact_case_exists(&dest) {
                Ok(Some(false)) => item.blocked_reason = Some("caseMismatch".into()),
                _ => item.action = "create".into(),
            },
            Err(e) => item.blocked_reason = Some(format!("unreadable: {e}")),
        }
        items.push(item);
    }
    let count = |a: &str| items.iter().filter(|i| i.action == a).count();
    Ok(RestorePreview {
        id: uuid::Uuid::now_v7().to_string(),
        snapshot_id: m.snapshot_id.clone(),
        worktree_path: paths::display(worktree),
        variant,
        created_at: crate::now_ms(),
        overwrite_count: count("overwrite"),
        create_count: count("create"),
        blocked_count: count("blocked"),
        items,
    })
}

/// Current fingerprint of a restore target (None = absent).
fn current_sha(dest: &Path) -> Result<Option<String>> {
    match fs::symlink_metadata(dest) {
        Ok(meta) if meta.is_file() && !paths::is_link_like(&meta) => Ok(Some(hash_file(dest)?.0)),
        Ok(_) => Err(CoreError::RecoveryAborted(format!(
            "{} is no longer a regular file",
            paths::display(dest)
        ))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(CoreError::io(dest, e)),
    }
}

/// Execute a previously built preview. `protect` must capture the listed
/// paths into a verified snapshot and return it.
pub fn execute_in_place(
    preview: &RestorePreview,
    blobs: &BlobStore,
    worktree: &Path,
    protect: &dyn Fn(&[String]) -> Result<SnapshotSummary>,
) -> Result<RecoveryReport> {
    let mut report = RecoveryReport::new(
        "restoreSelectedFiles",
        "snapshot",
        "verifiedLocalSnapshot",
        Some(preview.snapshot_id.clone()),
        &paths::display(worktree),
    );
    if !paths::same_path(worktree, Path::new(&preview.worktree_path)) {
        return Err(CoreError::RecoveryAborted(
            "worktree does not match the preview".into(),
        ));
    }
    let actionable: Vec<&PreviewItem> = preview
        .items
        .iter()
        .filter(|i| i.action == "overwrite" || i.action == "create")
        .collect();

    // Phase 1: source verification.
    for it in &actionable {
        if let Some(sha) = &it.snapshot_sha256
            && let Err(e) = blobs.verify(sha)
        {
            report.phase("sourceVerification", false, e.to_string());
            report.status = "aborted".into();
            report.finish();
            return Ok(report);
        }
    }
    report.phase(
        "sourceVerification",
        true,
        format!("{} blobs re-hashed", actionable.len()),
    );

    // Phase 2/3: target & conflict re-check against the preview.
    let mut changed = Vec::new();
    for it in &actionable {
        let dest = paths::safe_join(worktree, &it.path)?;
        paths::ensure_no_link_ancestors(worktree, &it.path)?;
        let now = current_sha(&dest)?;
        if now != it.current_sha256 {
            changed.push(it.path.clone());
        }
    }
    if !changed.is_empty() {
        report.phase(
            "conflictDetection",
            false,
            format!("changed since preview: {}", changed.join(", ")),
        );
        report.status = "aborted".into();
        report.notes.push("previewStale".into());
        report.finish();
        return Ok(report);
    }
    report.phase("targetSelection", true, paths::display(worktree));
    report.phase("conflictDetection", true, "targets unchanged since preview");

    // Phase 4: protection snapshot of everything we are about to replace.
    let to_protect: Vec<String> = actionable
        .iter()
        .filter(|i| i.action == "overwrite")
        .map(|i| i.path.clone())
        .collect();
    if !to_protect.is_empty() {
        match protect(&to_protect) {
            Ok(snap) if snap.status == crate::types::SnapshotStatus::Verified => {
                report.protection_snapshot_id = Some(snap.id.clone());
                report.phase(
                    "changePreview",
                    true,
                    format!("protection snapshot {}", snap.id),
                );
            }
            Ok(snap) => {
                report.phase(
                    "changePreview",
                    false,
                    format!("protection snapshot {} was not fully verified", snap.id),
                );
                report.status = "aborted".into();
                report.finish();
                return Ok(report);
            }
            Err(e) => {
                report.phase(
                    "changePreview",
                    false,
                    format!("protection snapshot failed: {e}"),
                );
                report.status = "aborted".into();
                report.finish();
                return Ok(report);
            }
        }
    } else {
        report.phase("changePreview", true, "no existing files are overwritten");
    }

    // Phase 5: execution.
    for it in &actionable {
        let sha = it.snapshot_sha256.clone().expect("actionable has content");
        let mut res = ItemResult {
            path: it.path.clone(),
            outcome: "failed".into(),
            detail: None,
            variant: Some(match preview.variant {
                Variant::WorkingTree => "workingTree".into(),
                Variant::Staged => "staged".into(),
            }),
            expected_sha256: Some(sha.clone()),
            written_sha256: None,
            bytes: None,
            destination: None,
            provenance: "verifiedLocalSnapshot".into(),
        };
        match restore_one(
            blobs,
            worktree,
            &it.path,
            &sha,
            it.current_sha256.as_deref(),
        ) {
            Ok((dest, n, h)) => {
                res.outcome = "written".into();
                res.bytes = Some(n);
                res.written_sha256 = Some(h);
                res.destination = Some(paths::display(&dest));
            }
            Err(e) => res.detail = Some(e.to_string()),
        }
        report.push(res);
    }
    for it in preview
        .items
        .iter()
        .filter(|i| i.action == "identical" || i.action == "blocked")
    {
        report.push(ItemResult {
            path: it.path.clone(),
            outcome: "skipped".into(),
            detail: Some(
                it.blocked_reason
                    .clone()
                    .unwrap_or_else(|| "identical".into()),
            ),
            variant: None,
            expected_sha256: it.snapshot_sha256.clone(),
            written_sha256: None,
            bytes: None,
            destination: None,
            provenance: "verifiedLocalSnapshot".into(),
        });
    }
    report.phase(
        "recoveryExecution",
        report.failed == 0,
        format!("{} written, {} failed", report.written, report.failed),
    );
    let mismatches = report
        .items
        .iter()
        .filter(|i| i.outcome == "written" && i.written_sha256 != i.expected_sha256)
        .count();
    report.phase(
        "postRecoveryValidation",
        mismatches == 0,
        format!("{mismatches} mismatches"),
    );
    report.notes.push("gitIndexUntouched".into());
    report.finish();
    Ok(report)
}

/// Write one file via temp + rename, re-checking the target immediately
/// before replacement, then re-hash.
fn restore_one(
    blobs: &BlobStore,
    worktree: &Path,
    rel: &str,
    sha: &str,
    expected_current: Option<&str>,
) -> Result<(PathBuf, u64, String)> {
    let dest = paths::safe_join(worktree, rel)?;
    paths::ensure_no_link_ancestors(worktree, rel)?;
    let parent = dest.parent().expect("joined path has parent").to_path_buf();
    fs::create_dir_all(&parent).map_err(|e| CoreError::io(&parent, e))?;
    paths::ensure_no_link_ancestors(worktree, rel)?;
    let tmp = parent.join(format!(
        ".wr-restore-{}.part",
        uuid::Uuid::new_v4().simple()
    ));
    let data = blobs.read(sha)?;
    if crate::blob::hash_bytes(&data) != sha {
        return Err(CoreError::Integrity {
            what: format!("blob {sha}"),
            expected: sha.into(),
            actual: crate::blob::hash_bytes(&data),
        });
    }
    {
        let mut f = fs::File::create_new(&tmp).map_err(|e| CoreError::io(&tmp, e))?;
        f.write_all(&data).map_err(|e| CoreError::io(&tmp, e))?;
        f.sync_all().map_err(|e| CoreError::io(&tmp, e))?;
    }
    // Last-moment race check.
    let now = match current_sha(&dest) {
        Ok(v) => v,
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
    };
    if now.as_deref() != expected_current {
        let _ = fs::remove_file(&tmp);
        return Err(CoreError::RecoveryAborted(format!(
            "{rel} changed during restore"
        )));
    }
    if let Err(e) = fs::rename(&tmp, &dest) {
        let _ = fs::remove_file(&tmp);
        return Err(CoreError::io(&dest, e));
    }
    let (h, n) = hash_file(&dest)?;
    if h != sha {
        return Err(CoreError::Integrity {
            what: format!("restored {rel}"),
            expected: sha.into(),
            actual: h,
        });
    }
    Ok((dest, n, h))
}
