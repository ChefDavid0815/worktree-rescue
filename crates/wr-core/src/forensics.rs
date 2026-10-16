//! Forensic Discovery: read-only search of Git's own recovery sources
//! (reflog, stash list, unreachable objects). Results are labelled with
//! their provenance and are never presented as Worktree Rescue snapshots.

use crate::error::{CoreError, Result};
use crate::git::{Git, RunOpts};
use crate::paths;
use crate::recovery::{ItemResult, RecoveryReport, check_separate_target, write_report_files};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForensicCandidate {
    /// gitReflog | gitStash | unreachableCommit | unreachableBlob
    pub provenance: String,
    pub oid: String,
    pub object_type: String,
    pub reference: Option<String>,
    pub subject: Option<String>,
    pub author_time: Option<i64>,
    pub size: Option<u64>,
    /// First bytes of a blob for identification (text only).
    pub preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForensicReport {
    pub scanned_at: i64,
    pub candidates: Vec<ForensicCandidate>,
    pub reflog_entries: usize,
    pub stash_entries: usize,
    pub unreachable_commits: usize,
    pub unreachable_blobs: usize,
    pub notes: Vec<String>,
}

const LOG_FMT: &str = "--format=%H%x1f%gD%x1f%ct%x1f%s";

fn parse_log(out: &[u8], provenance: &str) -> Vec<ForensicCandidate> {
    String::from_utf8_lossy(out)
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\x1f').collect();
            if f.len() < 4 {
                return None;
            }
            Some(ForensicCandidate {
                provenance: provenance.into(),
                oid: f[0].to_string(),
                object_type: "commit".into(),
                reference: Some(f[1].to_string()).filter(|s| !s.is_empty()),
                subject: Some(f[3].to_string()),
                author_time: f[2].parse::<i64>().ok().map(|s| s * 1000),
                size: None,
                preview: None,
            })
        })
        .collect()
}

/// Run the read-only discovery. `deep` adds `git fsck --unreachable`,
/// which can take a while on large repositories.
pub fn discover(
    git: &Git,
    repo: &Path,
    deep: bool,
    cancel: Option<&AtomicBool>,
) -> Result<ForensicReport> {
    let mut report = ForensicReport {
        scanned_at: crate::now_ms(),
        candidates: vec![],
        reflog_entries: 0,
        stash_entries: 0,
        unreachable_commits: 0,
        unreachable_blobs: 0,
        notes: vec![],
    };
    // Reflog of HEAD (all worktrees share refs but HEAD reflogs are per worktree).
    match git.run_opts(
        repo,
        &[
            "log",
            "-g",
            "--no-abbrev-commit",
            LOG_FMT,
            "-n",
            "300",
            "HEAD",
        ],
        RunOpts {
            ok_codes: &[128],
            cancel,
            ..Default::default()
        },
    ) {
        Ok(out) if out.code == 0 => {
            let c = parse_log(&out.stdout, "gitReflog");
            report.reflog_entries = c.len();
            report.candidates.extend(c);
        }
        Ok(_) => report.notes.push("noHeadReflog".into()),
        Err(e) => report.notes.push(format!("reflogFailed:{e}")),
    }
    match git.run_opts(
        repo,
        &["stash", "list", LOG_FMT],
        RunOpts {
            cancel,
            ..Default::default()
        },
    ) {
        Ok(out) => {
            let c = parse_log(&out.stdout, "gitStash");
            report.stash_entries = c.len();
            report.candidates.extend(c);
        }
        Err(e) => report.notes.push(format!("stashListFailed:{e}")),
    }
    if deep {
        match git.run_opts(
            repo,
            &["fsck", "--unreachable", "--no-reflogs", "--no-progress"],
            RunOpts {
                cancel,
                timeout: Some(Duration::from_secs(600)),
                ok_codes: &[1, 2, 3, 4, 5, 6, 7],
                ..Default::default()
            },
        ) {
            Ok(out) => {
                let mut commits = Vec::new();
                let mut blobs = Vec::new();
                for line in String::from_utf8_lossy(&out.stdout).lines() {
                    let mut it = line.split_whitespace();
                    if it.next() != Some("unreachable") {
                        continue;
                    }
                    match (it.next(), it.next()) {
                        (Some("commit"), Some(oid)) => commits.push(oid.to_string()),
                        (Some("blob"), Some(oid)) => blobs.push(oid.to_string()),
                        _ => {}
                    }
                }
                report.unreachable_commits = commits.len();
                report.unreachable_blobs = blobs.len();
                commits.truncate(300);
                blobs.truncate(500);
                if !commits.is_empty() {
                    let mut args = vec!["log", "--no-walk", "--no-abbrev-commit", LOG_FMT];
                    let refs: Vec<&str> = commits.iter().map(String::as_str).collect();
                    args.extend(refs);
                    if let Ok(out) = git.run(repo, &args) {
                        report
                            .candidates
                            .extend(parse_log(&out, "unreachableCommit"));
                    }
                }
                if !blobs.is_empty()
                    && let Ok(objs) = git.cat_file_batch(repo, &blobs, cancel)
                {
                    for (oid, obj) in objs {
                        if let Some((kind, bytes)) = obj {
                            let text = !bytes.iter().take(8000).any(|b| *b == 0);
                            report.candidates.push(ForensicCandidate {
                                provenance: "unreachableBlob".into(),
                                oid,
                                object_type: kind,
                                reference: None,
                                subject: None,
                                author_time: None,
                                size: Some(bytes.len() as u64),
                                preview: text.then(|| {
                                    String::from_utf8_lossy(&bytes[..bytes.len().min(400)])
                                        .into_owned()
                                }),
                            });
                        }
                    }
                }
            }
            Err(CoreError::Cancelled) => return Err(CoreError::Cancelled),
            Err(e) => report.notes.push(format!("fsckFailed:{e}")),
        }
    }
    Ok(report)
}

/// List files changed by a commit (for previewing a reflog/stash candidate).
pub fn commit_files(git: &Git, repo: &Path, oid: &str) -> Result<Vec<(String, String)>> {
    check_oid(oid)?;
    let out = git.run(
        repo,
        &[
            "diff",
            "--no-ext-diff",
            "--name-status",
            "-z",
            "--root",
            "-r",
            &format!("{oid}^!"),
        ],
    );
    let out = match out {
        Ok(o) => o,
        // Root commit: compare against the empty tree.
        Err(_) => git.run(repo, &["ls-tree", "-r", "-z", "--name-only", oid])?,
    };
    let parts: Vec<String> = out
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect();
    let mut files = Vec::new();
    let mut i = 0;
    while i < parts.len() {
        let status = parts[i].clone();
        if status.len() <= 3
            && status
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_uppercase())
        {
            if status.starts_with('R') || status.starts_with('C') {
                if i + 2 < parts.len() {
                    files.push((status, parts[i + 2].clone()));
                }
                i += 3;
            } else {
                if i + 1 < parts.len() {
                    files.push((status, parts[i + 1].clone()));
                }
                i += 2;
            }
        } else {
            files.push(("A".into(), status));
            i += 1;
        }
    }
    Ok(files)
}

pub fn check_oid(oid: &str) -> Result<()> {
    if (oid.len() == 40 || oid.len() == 64) && oid.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(CoreError::Other(format!("invalid object id {oid}")))
    }
}

/// Git's own object id for blob content (SHA-1 repositories).
pub fn git_blob_oid(data: &[u8]) -> String {
    use sha1::{Digest, Sha1};
    let mut h = Sha1::new();
    h.update(format!("blob {}\0", data.len()).as_bytes());
    h.update(data);
    hex::encode(h.finalize())
}

/// Read a blob from Git and verify it hashes to its object id.
pub fn read_git_blob(git: &Git, repo: &Path, oid: &str) -> Result<Vec<u8>> {
    check_oid(oid)?;
    let objs = git.cat_file_batch(repo, &[oid.to_string()], None)?;
    match objs.into_iter().next().and_then(|(_, o)| o) {
        Some((kind, bytes)) if kind == "blob" => {
            if oid.len() == 40 && git_blob_oid(&bytes) != oid.to_ascii_lowercase() {
                return Err(CoreError::Integrity {
                    what: format!("git blob {oid}"),
                    expected: oid.into(),
                    actual: git_blob_oid(&bytes),
                });
            }
            Ok(bytes)
        }
        Some((kind, _)) => Err(CoreError::Other(format!("{oid} is a {kind}, not a blob"))),
        None => Err(CoreError::Other(format!("object {oid} not found"))),
    }
}

fn write_verified(dest: &Path, data: &[u8]) -> Result<String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CoreError::io(parent, e))?;
    }
    if dest.exists() {
        return Err(CoreError::InvalidTarget(format!(
            "{} exists",
            dest.display()
        )));
    }
    let tmp = dest.with_extension("wr-part");
    std::fs::write(&tmp, data).map_err(|e| CoreError::io(&tmp, e))?;
    std::fs::rename(&tmp, dest).map_err(|e| CoreError::io(dest, e))?;
    let (h, _) = crate::blob::hash_file(dest)?;
    Ok(h)
}

/// Materialise every file of a commit's tree below `root`, skipping
/// `exclude` (paths already provided by a snapshot). Each blob is verified
/// against its Git object id before being written.
pub fn materialize_tree(
    git: &Git,
    repo: &Path,
    commit: &str,
    root: &Path,
    exclude: &[String],
    provenance: &str,
) -> Result<Vec<ItemResult>> {
    check_oid(commit)?;
    let out = git.run(repo, &["ls-tree", "-r", "-z", "--full-tree", commit])?;
    let mut items = Vec::new();
    let mut wanted: Vec<(String, String)> = Vec::new();
    for rec in out.split(|b| *b == 0).filter(|s| !s.is_empty()) {
        let line = String::from_utf8_lossy(rec);
        let Some((meta, path)) = line.split_once('\t') else {
            continue;
        };
        let f: Vec<&str> = meta.split(' ').collect();
        if f.len() != 3 {
            continue;
        }
        if exclude.iter().any(|e| e == path) {
            continue;
        }
        if f[1] != "blob" || f[0] == "120000" {
            items.push(ItemResult {
                path: path.to_string(),
                outcome: "skipped".into(),
                detail: Some(if f[0] == "120000" {
                    "symlinkListed".into()
                } else {
                    "submodule".into()
                }),
                variant: Some("headCommit".into()),
                expected_sha256: None,
                written_sha256: None,
                bytes: None,
                destination: None,
                provenance: provenance.into(),
            });
            continue;
        }
        wanted.push((path.to_string(), f[2].to_string()));
    }
    for chunk in wanted.chunks(200) {
        let oids: Vec<String> = chunk.iter().map(|(_, o)| o.clone()).collect();
        let objs = git.cat_file_batch(repo, &oids, None)?;
        for ((path, oid), (_, obj)) in chunk.iter().zip(objs) {
            let mut item = ItemResult {
                path: path.clone(),
                outcome: "failed".into(),
                detail: None,
                variant: Some("headCommit".into()),
                expected_sha256: None,
                written_sha256: None,
                bytes: None,
                destination: None,
                provenance: provenance.into(),
            };
            match obj {
                Some((_, bytes)) => {
                    if oid.len() == 40 && git_blob_oid(&bytes) != *oid {
                        item.detail = Some("gitObjectHashMismatch".into());
                    } else {
                        match paths::safe_join(root, path) {
                            Ok(dest) => match write_verified(&dest, &bytes) {
                                Ok(h) => {
                                    item.outcome = "written".into();
                                    item.expected_sha256 = Some(crate::blob::hash_bytes(&bytes));
                                    item.written_sha256 = Some(h);
                                    item.bytes = Some(bytes.len() as u64);
                                    item.destination = Some(paths::display(&dest));
                                }
                                Err(e) => item.detail = Some(e.to_string()),
                            },
                            Err(e) => item.detail = Some(e.to_string()),
                        }
                    }
                }
                None => item.detail = Some("objectMissing".into()),
            }
            items.push(item);
        }
    }
    Ok(items)
}

/// Recover a forensic candidate (commit tree or single blob) into a new
/// directory.
pub fn recover_object(
    git: &Git,
    repo: &Path,
    candidate: &ForensicCandidate,
    target: &Path,
    protected_roots: &[PathBuf],
) -> Result<RecoveryReport> {
    let mut report = RecoveryReport::new(
        "separateDirectory",
        "gitObject",
        &candidate.provenance,
        None,
        &paths::display(target),
    );
    report.git_context = serde_json::to_value(candidate)?;
    check_oid(&candidate.oid)?;
    if let Err(e) = check_separate_target(target, protected_roots) {
        report.phase("targetSelection", false, e.to_string());
        report.status = "aborted".into();
        report.finish();
        return Ok(report);
    }
    report.phase("targetSelection", true, paths::display(target));
    let short = &candidate.oid[..12];
    if candidate.object_type == "blob" {
        match read_git_blob(git, repo, &candidate.oid) {
            Ok(bytes) => {
                report.phase("sourceVerification", true, "object id verified");
                let dest = target.join(format!("blob-{short}.bin"));
                let mut item = ItemResult {
                    path: format!("blob-{short}.bin"),
                    outcome: "failed".into(),
                    detail: None,
                    variant: None,
                    expected_sha256: Some(crate::blob::hash_bytes(&bytes)),
                    written_sha256: None,
                    bytes: Some(bytes.len() as u64),
                    destination: None,
                    provenance: candidate.provenance.clone(),
                };
                match write_verified(&dest, &bytes) {
                    Ok(h) => {
                        item.outcome = "written".into();
                        item.written_sha256 = Some(h);
                        item.destination = Some(paths::display(&dest));
                    }
                    Err(e) => item.detail = Some(e.to_string()),
                }
                report.push(item);
            }
            Err(e) => {
                report.phase("sourceVerification", false, e.to_string());
                report.status = "aborted".into();
                report.finish();
                return Ok(report);
            }
        }
    } else {
        report.phase(
            "sourceVerification",
            true,
            "objects verified individually while writing",
        );
        let root = target.join(format!("commit-{short}"));
        let items = materialize_tree(git, repo, &candidate.oid, &root, &[], &candidate.provenance)?;
        for it in items {
            report.push(it);
        }
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
    report.finish();
    write_report_files(target, &report)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_blob_hash_matches_git() {
        // `printf 'hello\n' | git hash-object --stdin`
        assert_eq!(
            git_blob_oid(b"hello\n"),
            "ce013625030ba8dba906f756967f9e9ca394464a"
        );
    }
}
