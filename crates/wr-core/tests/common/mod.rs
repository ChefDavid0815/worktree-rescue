#![allow(dead_code)]
//! Shared fixtures: real Git repositories in temporary directories.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use wr_core::types::{NoProgress, SnapshotSummary, SnapshotTrigger};
use wr_core::vault::Vault;

pub fn git(cwd: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "core.autocrlf=false",
        ])
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

pub fn write(p: &Path, content: &[u8]) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}

pub struct Fixture {
    pub _dir: tempfile::TempDir,
    pub base: PathBuf,
    pub repo: PathBuf,
    pub vault: Vault,
}

impl Fixture {
    /// A repository with two commits and a clean working tree.
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().to_path_buf();
        let repo = base.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        write(&repo.join("README.md"), b"# fixture\n");
        write(&repo.join("src/lib.rs"), b"pub fn a() -> u32 { 1 }\n");
        write(&repo.join("src/old_name.rs"), b"pub fn renamed() {}\n");
        write(&repo.join("docs/notes.txt"), b"line 1\nline 2\n");
        write(&repo.join(".gitignore"), b"target/\n*.log\n");
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "initial"]);
        write(&repo.join("CHANGELOG.md"), b"- initial\n");
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "second"]);
        let vault = Vault::open(&base.join("vault"), false).unwrap();
        Fixture {
            _dir: dir,
            base,
            repo,
            vault,
        }
    }

    pub fn add_project(&self) -> String {
        self.vault.add_project(&self.repo, Some(false)).unwrap().id
    }

    pub fn snapshot(&self, project: &str, wt: &Path) -> SnapshotSummary {
        self.snapshot_with(project, wt, SnapshotTrigger::Manual)
            .expect("snapshot created")
    }

    pub fn snapshot_with(
        &self,
        project: &str,
        wt: &Path,
        trigger: SnapshotTrigger,
    ) -> Option<SnapshotSummary> {
        let cancel = AtomicBool::new(false);
        let wt = PathBuf::from(wr_core::paths::display(wt));
        self.vault
            .create_snapshot(project, &wt, trigger, None, &cancel, &NoProgress, "test")
            .unwrap()
            .snapshot
    }

    /// Fingerprint of everything Worktree Rescue promises not to change:
    /// working tree bytes, index, HEAD, refs and reflogs.
    pub fn fingerprint(&self, wt: &Path) -> BTreeMap<String, String> {
        fingerprint(wt)
    }
}

pub fn sha(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

pub fn fingerprint(wt: &Path) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    walk(wt, wt, &mut map);
    let common = PathBuf::from(
        git(
            wt,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )
        .trim(),
    );
    let gitdir =
        PathBuf::from(git(wt, &["rev-parse", "--path-format=absolute", "--git-dir"]).trim());
    for (label, p) in [
        ("index", gitdir.join("index")),
        ("HEAD", gitdir.join("HEAD")),
        ("packed-refs", common.join("packed-refs")),
    ] {
        if let Ok(b) = std::fs::read(&p) {
            map.insert(format!("@{label}"), sha(&b));
        }
    }
    map.insert(
        "@refs".into(),
        sha(git(wt, &["for-each-ref", "--format=%(refname) %(objectname)"]).as_bytes()),
    );
    let mut logs = BTreeMap::new();
    walk(&common.join("logs"), &common.join("logs"), &mut logs);
    map.insert("@reflogs".into(), sha(format!("{logs:?}").as_bytes()));
    map.insert("@stash".into(), sha(git(wt, &["stash", "list"]).as_bytes()));
    map
}

fn walk(root: &Path, dir: &Path, map: &mut BTreeMap<String, String>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if e.file_name() == ".git" && p.is_dir() {
            continue;
        }
        let meta = std::fs::symlink_metadata(&p).unwrap();
        let rel = p
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if meta.is_dir() {
            walk(root, &p, map);
        } else {
            let b = std::fs::read(&p).unwrap_or_default();
            map.insert(rel, sha(&b));
        }
    }
}
