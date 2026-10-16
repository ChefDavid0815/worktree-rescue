//! Snapshot manifests: the self-describing record that ties paths, Git
//! context and blob hashes together. A manifest plus the blob store is
//! sufficient to rebuild every captured file without the database.

use crate::blob::{BlobStore, hash_bytes};
use crate::error::{CoreError, Result};
use crate::paths::validate_rel_path;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GitContext {
    pub branch: Option<String>,
    pub head_oid: Option<String>,
    pub detached: bool,
    pub unborn: bool,
    pub upstream: Option<String>,
    pub git_dir: String,
    pub common_dir: String,
    pub sparse_checkout: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IndexState {
    /// Git object id of the staged blob.
    pub git_oid: String,
    /// Octal Git mode (100644, 100755, 120000, 160000).
    pub mode: u32,
    /// Vault blob holding the staged bytes (None for submodule gitlinks or
    /// when the staged object could not be read).
    pub blob: Option<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WtState {
    /// Bytes captured in `blob`.
    Present,
    /// File does not exist in the working tree (deleted).
    Absent,
    /// Intentionally not captured (see `skipReason`).
    Skipped,
    /// Capture attempted but failed (see `skipReason`).
    Failed,
}

impl WtState {
    pub fn as_str(&self) -> &'static str {
        match self {
            WtState::Present => "present",
            WtState::Absent => "absent",
            WtState::Skipped => "skipped",
            WtState::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EntryKind {
    File,
    Symlink,
    Submodule,
    Other,
}

impl EntryKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EntryKind::File => "file",
            EntryKind::Symlink => "symlink",
            EntryKind::Submodule => "submodule",
            EntryKind::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeState {
    pub state: WtState,
    pub kind: EntryKind,
    pub blob: Option<String>,
    pub size: Option<u64>,
    pub executable: bool,
    pub mtime_ms: Option<i64>,
    /// For symlinks: the link text (never followed).
    pub link_target: Option<String>,
    /// For symlinks: whether the target resolves outside the worktree.
    pub link_escapes: bool,
    pub skip_reason: Option<String>,
    pub lfs_pointer: bool,
    pub binary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManifestEntry {
    /// Repository-relative path, forward slashes.
    pub path: String,
    pub orig_path: Option<String>,
    /// modified | added | deleted | renamed | copied | typeChanged |
    /// untracked | conflicted | ignoredIncluded
    pub category: String,
    pub x: String,
    pub y: String,
    pub head_oid: Option<String>,
    pub head_mode: Option<u32>,
    pub index: Option<IndexState>,
    pub worktree: WorktreeState,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManifestStats {
    pub entries: u64,
    pub captured: u64,
    pub skipped: u64,
    pub failed: u64,
    pub protected_bytes: u64,
    pub new_bytes: u64,
    pub ignored_not_protected: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub format_version: u32,
    pub snapshot_id: String,
    pub project_id: String,
    pub created_at: i64,
    pub finished_at: i64,
    pub trigger: String,
    pub worktree_path: String,
    pub git: GitContext,
    pub tool_version: String,
    pub policy: serde_json::Value,
    pub entries: Vec<ManifestEntry>,
    pub stats: ManifestStats,
}

impl Manifest {
    /// Structural validation: version, unique safe paths, well-formed hashes.
    pub fn validate(&self) -> Result<()> {
        if self.format_version != FORMAT_VERSION {
            return Err(CoreError::InvalidManifest(format!(
                "unsupported format version {}",
                self.format_version
            )));
        }
        let mut seen = std::collections::HashSet::new();
        for e in &self.entries {
            validate_rel_path(&e.path)
                .map_err(|err| CoreError::InvalidManifest(err.to_string()))?;
            if let Some(o) = &e.orig_path {
                validate_rel_path(o).map_err(|err| CoreError::InvalidManifest(err.to_string()))?;
            }
            if !seen.insert(e.path.as_str()) {
                return Err(CoreError::InvalidManifest(format!(
                    "duplicate path {}",
                    e.path
                )));
            }
            let check = |h: &Option<String>| -> Result<()> {
                if let Some(h) = h
                    && (h.len() != 64 || !h.bytes().all(|b| b.is_ascii_hexdigit()))
                {
                    return Err(CoreError::InvalidManifest(format!(
                        "bad hash for {}",
                        e.path
                    )));
                }
                Ok(())
            };
            check(&e.worktree.blob)?;
            if let Some(i) = &e.index {
                check(&i.blob)?;
            }
            if e.worktree.state == WtState::Present
                && e.worktree.kind == EntryKind::File
                && e.worktree.blob.is_none()
            {
                return Err(CoreError::InvalidManifest(format!(
                    "present file without blob: {}",
                    e.path
                )));
            }
        }
        Ok(())
    }

    /// All blob ids referenced by this manifest.
    pub fn blob_refs(&self) -> Vec<String> {
        let mut v = Vec::new();
        for e in &self.entries {
            if let Some(b) = &e.worktree.blob {
                v.push(b.clone());
            }
            if let Some(i) = &e.index
                && let Some(b) = &i.blob
            {
                v.push(b.clone());
            }
        }
        v.sort();
        v.dedup();
        v
    }

    /// Verify every referenced blob exists. With `deep`, re-hash content.
    pub fn verify_blobs(&self, store: &BlobStore, deep: bool) -> Vec<(String, String)> {
        let mut problems = Vec::new();
        for b in self.blob_refs() {
            if deep {
                if let Err(e) = store.verify(&b) {
                    problems.push((b, e.to_string()));
                }
            } else if !store.contains(&b) {
                problems.push((b, "missing".into()));
            }
        }
        problems
    }
}

/// On-disk manifest location: `<root>/<id[0..2]>/<id>.json`.
pub fn manifest_path(root: &Path, snapshot_id: &str) -> Result<PathBuf> {
    if snapshot_id.len() < 8
        || !snapshot_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(CoreError::Other(format!("bad snapshot id {snapshot_id}")));
    }
    Ok(root
        .join(&snapshot_id[0..2])
        .join(format!("{snapshot_id}.json")))
}

/// Atomically write a manifest; returns its SHA-256.
pub fn write_manifest(root: &Path, m: &Manifest) -> Result<String> {
    m.validate()?;
    let bytes = serde_json::to_vec_pretty(m)?;
    let sha = hash_bytes(&bytes);
    let dest = manifest_path(root, &m.snapshot_id)?;
    let dir = dest.parent().expect("manifest has parent");
    fs::create_dir_all(dir).map_err(|e| CoreError::io(dir, e))?;
    let tmp = dir.join(format!(".{}.part", m.snapshot_id));
    {
        let mut f = fs::File::create(&tmp).map_err(|e| CoreError::io(&tmp, e))?;
        f.write_all(&bytes).map_err(|e| CoreError::io(&tmp, e))?;
        f.sync_all().map_err(|e| CoreError::io(&tmp, e))?;
    }
    fs::rename(&tmp, &dest).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        CoreError::io(&dest, e)
    })?;
    Ok(sha)
}

/// Read a manifest and, when `expected_sha` is given, check its integrity.
pub fn read_manifest(
    root: &Path,
    snapshot_id: &str,
    expected_sha: Option<&str>,
) -> Result<Manifest> {
    let p = manifest_path(root, snapshot_id)?;
    let bytes = fs::read(&p).map_err(|e| CoreError::io(&p, e))?;
    if let Some(exp) = expected_sha {
        let actual = hash_bytes(&bytes);
        if !actual.eq_ignore_ascii_case(exp) {
            return Err(CoreError::Integrity {
                what: format!("manifest {snapshot_id}"),
                expected: exp.to_string(),
                actual,
            });
        }
    }
    let m: Manifest = serde_json::from_slice(&bytes)?;
    m.validate()?;
    Ok(m)
}

pub fn delete_manifest(root: &Path, snapshot_id: &str) -> Result<()> {
    let p = manifest_path(root, snapshot_id)?;
    match fs::remove_file(&p) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(CoreError::io(&p, e)),
    }
}

/// Heuristic binary detection: NUL byte in the first 8 KB.
pub fn looks_binary(prefix: &[u8]) -> bool {
    prefix.iter().take(8000).any(|b| *b == 0)
}

pub fn is_lfs_pointer(prefix: &[u8]) -> bool {
    prefix.starts_with(b"version https://git-lfs.github.com/spec/v1")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Manifest {
        Manifest {
            format_version: FORMAT_VERSION,
            snapshot_id: "0190aaaa-bbbb-7ccc-8ddd-eeeeffff0000".into(),
            project_id: "p".into(),
            created_at: 1,
            finished_at: 2,
            trigger: "manual".into(),
            worktree_path: "C:/r".into(),
            git: GitContext {
                branch: Some("main".into()),
                head_oid: None,
                detached: false,
                unborn: true,
                upstream: None,
                git_dir: "g".into(),
                common_dir: "g".into(),
                sparse_checkout: false,
            },
            tool_version: "test".into(),
            policy: serde_json::json!({}),
            entries: vec![ManifestEntry {
                path: "a.txt".into(),
                orig_path: None,
                category: "untracked".into(),
                x: "?".into(),
                y: "?".into(),
                head_oid: None,
                head_mode: None,
                index: None,
                worktree: WorktreeState {
                    state: WtState::Present,
                    kind: EntryKind::File,
                    blob: Some(hash_bytes(b"x")),
                    size: Some(1),
                    executable: false,
                    mtime_ms: None,
                    link_target: None,
                    link_escapes: false,
                    skip_reason: None,
                    lfs_pointer: false,
                    binary: false,
                },
            }],
            stats: ManifestStats::default(),
        }
    }

    #[test]
    fn roundtrip_and_tamper_detection() {
        let dir = tempfile::tempdir().unwrap();
        let m = sample();
        let sha = write_manifest(dir.path(), &m).unwrap();
        let back = read_manifest(dir.path(), &m.snapshot_id, Some(&sha)).unwrap();
        assert_eq!(back, m);
        let p = manifest_path(dir.path(), &m.snapshot_id).unwrap();
        let mut s = fs::read_to_string(&p).unwrap();
        s = s.replace("a.txt", "b.txt");
        fs::write(&p, s).unwrap();
        assert!(matches!(
            read_manifest(dir.path(), &m.snapshot_id, Some(&sha)),
            Err(CoreError::Integrity { .. })
        ));
    }

    #[test]
    fn rejects_traversal_entries() {
        let mut m = sample();
        m.entries[0].path = "../../evil".into();
        assert!(m.validate().is_err());
        let mut m = sample();
        m.entries.push(m.entries[0].clone());
        assert!(m.validate().is_err());
    }
}
