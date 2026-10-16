mod common;

use common::*;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use wr_core::recovery::Variant;
use wr_core::settings::Settings;
use wr_core::types::*;

fn file<'a>(files: &'a [SnapshotFile], path: &str) -> &'a SnapshotFile {
    files.iter().find(|f| f.path == path).unwrap_or_else(|| {
        panic!(
            "{path} missing from snapshot: {:?}",
            files.iter().map(|f| &f.path).collect::<Vec<_>>()
        )
    })
}

/// Every kind of change is captured, and a separate-directory recovery
/// reproduces the exact bytes that were in the working tree.
#[test]
fn captures_all_change_kinds_and_recovers_exact_bytes() {
    let fx = Fixture::new();
    let r = &fx.repo;
    // unstaged modification
    write(&r.join("src/lib.rs"), b"pub fn a() -> u32 { 2 }\n");
    // staged then further modified (index != worktree)
    write(&r.join("README.md"), b"# fixture\nstaged line\n");
    git(r, &["add", "README.md"]);
    write(
        &r.join("README.md"),
        b"# fixture\nstaged line\nunstaged line\n",
    );
    // staged new file
    write(&r.join("src/new.rs"), b"pub fn new() {}\n");
    git(r, &["add", "src/new.rs"]);
    // deletion
    std::fs::remove_file(r.join("docs/notes.txt")).unwrap();
    // staged rename
    git(r, &["mv", "src/old_name.rs", "src/new_name.rs"]);
    // untracked with unicode + spaces
    write(&r.join("草稿/新 文件.md"), "中文内容\n".as_bytes());
    // ignored (not captured)
    write(&r.join("build.log"), b"log\n");
    // binary untracked
    write(&r.join("assets/blob.bin"), &[0u8, 1, 2, 3, 0, 255]);

    let pid = fx.add_project();
    let s = fx.snapshot(&pid, r);
    assert_eq!(s.status, SnapshotStatus::Verified, "{s:?}");
    let detail = fx.vault.snapshot_detail(&s.id).unwrap();
    assert!(detail.manifest_ok);
    let files = &detail.files;
    assert_eq!(file(files, "src/lib.rs").category, "modified");
    assert_eq!(file(files, "src/new.rs").category, "added");
    assert_eq!(file(files, "docs/notes.txt").wt_state, "absent");
    let ren = file(files, "src/new_name.rs");
    assert_eq!(ren.category, "renamed");
    assert_eq!(ren.orig_path.as_deref(), Some("src/old_name.rs"));
    assert_eq!(file(files, "草稿/新 文件.md").category, "untracked");
    assert!(file(files, "assets/blob.bin").binary);
    assert!(
        files.iter().all(|f| f.path != "build.log"),
        "ignored file must not be captured"
    );
    assert!(detail.ignored_not_protected >= 1);
    let readme = file(files, "README.md");
    assert!(readme.index_blob.is_some() && readme.index_blob != readme.wt_blob);

    // Recover into a fresh directory and compare hashes with the originals.
    let target = fx.base.join("recovered");
    let report = fx
        .vault
        .recover_to_directory(&s.id, &target, None, true, false, &[])
        .unwrap();
    assert_eq!(report.status, "completed", "{report:#?}");
    for rel in [
        "src/lib.rs",
        "README.md",
        "src/new.rs",
        "src/new_name.rs",
        "草稿/新 文件.md",
        "assets/blob.bin",
    ] {
        let orig = std::fs::read(r.join(rel)).unwrap();
        let rec = std::fs::read(target.join("working-tree").join(rel)).unwrap();
        assert_eq!(sha(&orig), sha(&rec), "{rel} differs after recovery");
    }
    // The staged README differs from the working tree and is exported too.
    let staged = std::fs::read(target.join("staged/README.md")).unwrap();
    assert_eq!(staged, b"# fixture\nstaged line\n");
    assert!(target.join("recovery-report.json").is_file());
    assert!(target.join("RECOVERY-README.md").is_file());
    assert!(!target.join("working-tree/docs/notes.txt").exists());
}

#[test]
fn materializes_head_files_with_git_provenance() {
    let fx = Fixture::new();
    write(&fx.repo.join("src/lib.rs"), b"changed\n");
    let pid = fx.add_project();
    let s = fx.snapshot(&pid, &fx.repo);
    let target = fx.base.join("full");
    let report = fx
        .vault
        .recover_to_directory(&s.id, &target, None, false, true, &[])
        .unwrap();
    assert_eq!(report.status, "completed", "{report:#?}");
    assert_eq!(
        std::fs::read(target.join("working-tree/src/lib.rs")).unwrap(),
        b"changed\n"
    );
    assert_eq!(
        std::fs::read(target.join("working-tree/CHANGELOG.md")).unwrap(),
        b"- initial\n"
    );
    let head_item = report
        .items
        .iter()
        .find(|i| i.path == "CHANGELOG.md")
        .unwrap();
    assert_eq!(head_item.provenance, "existingGitObject");
}

#[test]
fn detached_head_and_unborn_repositories() {
    let fx = Fixture::new();
    let head = git(&fx.repo, &["rev-parse", "HEAD~1"]);
    git(&fx.repo, &["checkout", "-q", "--detach", head.trim()]);
    write(&fx.repo.join("detached.txt"), b"x\n");
    let pid = fx.add_project();
    let s = fx.snapshot(&pid, &fx.repo);
    assert!(s.detached);
    assert_eq!(s.head_oid.as_deref(), Some(head.trim()));

    // Repository without any commit.
    let unborn = fx.base.join("unborn");
    std::fs::create_dir_all(&unborn).unwrap();
    git(&unborn, &["init", "-q", "-b", "main"]);
    write(&unborn.join("first.txt"), b"hello\n");
    write(&unborn.join("staged.txt"), b"staged\n");
    git(&unborn, &["add", "staged.txt"]);
    let p2 = fx.vault.add_project(&unborn, Some(false)).unwrap();
    let s2 = fx.snapshot(&p2.id, &unborn);
    assert_eq!(s2.status, SnapshotStatus::Verified);
    assert!(s2.head_oid.is_none());
    assert_eq!(s2.entry_count, 2);
}

#[test]
fn large_files_are_skipped_and_reported() {
    let fx = Fixture::new();
    fx.vault
        .update_settings(Settings {
            max_file_bytes: 2048,
            ..fx.vault.settings()
        })
        .unwrap();
    write(&fx.repo.join("big.bin"), &vec![7u8; 10_000]);
    write(&fx.repo.join("small.txt"), b"small\n");
    let pid = fx.add_project();
    let s = fx.snapshot(&pid, &fx.repo);
    // A policy skip is not a failure: the snapshot is verified but says so.
    assert_eq!(s.status, SnapshotStatus::Verified);
    assert_eq!(s.skipped_count, 1);
    let files = fx.vault.snapshot_detail(&s.id).unwrap().files;
    assert_eq!(
        file(&files, "big.bin").skip_reason.as_deref(),
        Some("tooLarge")
    );
    assert_eq!(file(&files, "small.txt").wt_state, "present");
}

#[test]
fn budget_exhaustion_yields_partial_snapshot() {
    let fx = Fixture::new();
    fx.vault
        .update_settings(Settings {
            repo_budget_bytes: 64 * 1024 * 1024,
            ..fx.vault.settings()
        })
        .unwrap();
    let pid = fx.add_project();
    // Fill the budget with one big snapshot first (incompressible data).
    let mut big = Vec::with_capacity(40 * 1024 * 1024);
    let mut x: u64 = 0x9E3779B97F4A7C15;
    while big.len() < 40 * 1024 * 1024 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        big.extend_from_slice(&x.to_le_bytes());
    }
    fx.vault
        .update_settings(Settings {
            max_file_bytes: 100 * 1024 * 1024,
            ..fx.vault.settings()
        })
        .unwrap();
    write(&fx.repo.join("a.bin"), &big);
    let s1 = fx.snapshot(&pid, &fx.repo);
    assert_eq!(s1.status, SnapshotStatus::Verified);
    big.reverse();
    write(&fx.repo.join("b.bin"), &big);
    let s2 = fx.snapshot(&pid, &fx.repo);
    assert_eq!(s2.status, SnapshotStatus::Partial, "{s2:?}");
    let files = fx.vault.snapshot_detail(&s2.id).unwrap().files;
    assert_eq!(
        file(&files, "b.bin").skip_reason.as_deref(),
        Some("budgetExceeded")
    );
}

#[test]
fn unchanged_auto_snapshots_are_not_duplicated() {
    let fx = Fixture::new();
    write(&fx.repo.join("x.txt"), b"x\n");
    let pid = fx.add_project();
    assert!(
        fx.snapshot_with(&pid, &fx.repo, SnapshotTrigger::Auto)
            .is_some()
    );
    assert!(
        fx.snapshot_with(&pid, &fx.repo, SnapshotTrigger::Auto)
            .is_none()
    );
    write(&fx.repo.join("x.txt"), b"y\n");
    assert!(
        fx.snapshot_with(&pid, &fx.repo, SnapshotTrigger::Auto)
            .is_some()
    );
    // Manual snapshots are always taken.
    assert!(
        fx.snapshot_with(&pid, &fx.repo, SnapshotTrigger::Manual)
            .is_some()
    );
}

#[test]
fn sensitive_files_excluded_by_default_and_opt_in() {
    let fx = Fixture::new();
    write(&fx.repo.join(".env"), b"SECRET=1\n");
    write(&fx.repo.join("node_modules/pkg/index.js"), b"x\n");
    let pid = fx.add_project();
    let s = fx.snapshot(&pid, &fx.repo);
    let files = fx.vault.snapshot_detail(&s.id).unwrap().files;
    assert_eq!(
        file(&files, ".env").skip_reason.as_deref(),
        Some("sensitive")
    );
    assert_eq!(
        file(&files, "node_modules/pkg/index.js")
            .skip_reason
            .as_deref(),
        Some("excluded")
    );
    fx.vault
        .update_settings(Settings {
            include_sensitive: true,
            ..fx.vault.settings()
        })
        .unwrap();
    let s2 = fx.snapshot(&pid, &fx.repo);
    let files = fx.vault.snapshot_detail(&s2.id).unwrap().files;
    assert_eq!(file(&files, ".env").wt_state, "present");
}

#[test]
fn ignored_files_only_with_explicit_include() {
    let fx = Fixture::new();
    write(&fx.repo.join("target/keep/notes.md"), b"keep me\n");
    write(&fx.repo.join("target/drop.o"), b"x\n");
    fx.vault
        .update_settings(Settings {
            include_ignored_globs: vec!["target/keep/**".into(), "*.log".into()],
            ..fx.vault.settings()
        })
        .unwrap();
    write(&fx.repo.join("debug.log"), b"log line\n");
    write(&fx.repo.join("other.tmp.log"), b"x\n");
    let pid = fx.add_project();
    let s = fx.snapshot(&pid, &fx.repo);
    let files = fx.vault.snapshot_detail(&s.id).unwrap().files;
    // The whole `target/` dir is ignored; the walker picks only the included part.
    assert_eq!(
        file(&files, "target/keep/notes.md").category,
        "ignoredIncluded"
    );
    assert_eq!(file(&files, "debug.log").wt_state, "present");
    assert!(files.iter().all(|f| f.path != "target/drop.o"));
}

#[test]
fn ignored_files_are_counted_but_not_protected_by_default() {
    let fx = Fixture::new();
    write(&fx.repo.join("debug.log"), b"x\n");
    write(&fx.repo.join("tracked-change.txt"), b"y\n");
    let pid = fx.add_project();
    let s = fx.snapshot(&pid, &fx.repo);
    let d = fx.vault.snapshot_detail(&s.id).unwrap();
    assert!(d.files.iter().all(|f| f.path != "debug.log"));
    assert_eq!(d.ignored_not_protected, 1);
}

#[test]
fn in_place_restore_with_protection_snapshot() {
    let fx = Fixture::new();
    write(&fx.repo.join("src/lib.rs"), b"version A\n");
    let pid = fx.add_project();
    let s = fx.snapshot(&pid, &fx.repo);
    // Agent overwrites the file afterwards.
    write(&fx.repo.join("src/lib.rs"), b"version B (agent)\n");
    let preview = fx
        .vault
        .preview_restore(&s.id, vec!["src/lib.rs".into()], Variant::WorkingTree)
        .unwrap();
    assert_eq!(preview.overwrite_count, 1);
    let cancel = AtomicBool::new(false);
    let report = fx
        .vault
        .execute_restore(&preview.id, &cancel, &NoProgress)
        .unwrap();
    assert_eq!(report.status, "completed", "{report:#?}");
    assert_eq!(
        std::fs::read(fx.repo.join("src/lib.rs")).unwrap(),
        b"version A\n"
    );
    // The overwritten content is preserved in a verified protection snapshot.
    let prot = report.protection_snapshot_id.expect("protection snapshot");
    let pdetail = fx.vault.snapshot_detail(&prot).unwrap();
    assert_eq!(pdetail.summary.trigger, SnapshotTrigger::PreRestore);
    let pf = file(&pdetail.files, "src/lib.rs");
    let content = fx
        .vault
        .blob_content(pf.wt_blob.as_ref().unwrap(), 1024)
        .unwrap();
    assert_eq!(content.text.as_deref(), Some("version B (agent)\n"));
    // A preview can only be used once.
    assert!(
        fx.vault
            .execute_restore(&preview.id, &cancel, &NoProgress)
            .is_err()
    );
}

#[test]
fn in_place_restore_aborts_when_target_changes_after_preview() {
    let fx = Fixture::new();
    write(&fx.repo.join("src/lib.rs"), b"snapshot\n");
    let pid = fx.add_project();
    let s = fx.snapshot(&pid, &fx.repo);
    write(&fx.repo.join("src/lib.rs"), b"current\n");
    let preview = fx
        .vault
        .preview_restore(&s.id, vec!["src/lib.rs".into()], Variant::WorkingTree)
        .unwrap();
    // Someone edits the file between preview and execution.
    write(&fx.repo.join("src/lib.rs"), b"edited after preview\n");
    let before = fx.fingerprint(&fx.repo);
    let cancel = AtomicBool::new(false);
    let report = fx
        .vault
        .execute_restore(&preview.id, &cancel, &NoProgress)
        .unwrap();
    assert_eq!(report.status, "aborted");
    assert_eq!(
        before,
        fx.fingerprint(&fx.repo),
        "aborted restore must not modify anything"
    );
}

#[test]
fn deletions_are_never_replayed_in_place() {
    let fx = Fixture::new();
    std::fs::remove_file(fx.repo.join("docs/notes.txt")).unwrap();
    let pid = fx.add_project();
    let s = fx.snapshot(&pid, &fx.repo);
    write(&fx.repo.join("docs/notes.txt"), b"recreated\n");
    let preview = fx
        .vault
        .preview_restore(&s.id, vec!["docs/notes.txt".into()], Variant::WorkingTree)
        .unwrap();
    assert_eq!(preview.blocked_count, 1);
    assert_eq!(
        preview.items[0].blocked_reason.as_deref(),
        Some("deletionNotReplayed")
    );
}

#[test]
fn separate_directory_target_rules() {
    let fx = Fixture::new();
    write(&fx.repo.join("a.txt"), b"a\n");
    let pid = fx.add_project();
    let s = fx.snapshot(&pid, &fx.repo);
    // Inside the worktree: refused.
    let inside = fx
        .vault
        .recover_to_directory(&s.id, &fx.repo.join("recovered"), None, false, false, &[])
        .unwrap();
    assert_eq!(inside.status, "aborted");
    assert!(!fx.repo.join("recovered").exists());
    // Non-empty directory: refused.
    let busy = fx.base.join("busy");
    write(&busy.join("existing.txt"), b"keep\n");
    let r = fx
        .vault
        .recover_to_directory(&s.id, &busy, None, false, false, &[])
        .unwrap();
    assert_eq!(r.status, "aborted");
    assert_eq!(std::fs::read(busy.join("existing.txt")).unwrap(), b"keep\n");
    // Relative path: refused.
    let rel = fx
        .vault
        .recover_to_directory(&s.id, &PathBuf::from("relative"), None, false, false, &[])
        .unwrap();
    assert_eq!(rel.status, "aborted");
}

#[test]
fn corruption_is_detected_and_blocks_recovery() {
    let fx = Fixture::new();
    write(&fx.repo.join("a.txt"), b"precious\n");
    let pid = fx.add_project();
    let s = fx.snapshot(&pid, &fx.repo);
    let files = fx.vault.snapshot_detail(&s.id).unwrap().files;
    let sha_a = file(&files, "a.txt").wt_blob.clone().unwrap();
    let p = fx.vault.blobs().path_for(&sha_a).unwrap();
    std::fs::write(&p, b"tampered\n").unwrap();
    let v = fx.vault.verify_snapshot(&s.id, true).unwrap();
    assert!(!v.ok);
    let risks = fx.vault.list_risks(&pid, false).unwrap();
    assert!(risks.iter().any(|r| r.rule == "snapshotIntegrity"));
    let target = fx.base.join("out");
    let report = fx
        .vault
        .recover_to_directory(&s.id, &target, None, false, false, &[])
        .unwrap();
    assert_eq!(report.status, "aborted");
    assert!(!target.join("working-tree/a.txt").exists());
}

#[test]
fn retention_and_gc_keep_shared_blobs() {
    let fx = Fixture::new();
    write(&fx.repo.join("shared.txt"), b"shared content\n");
    write(&fx.repo.join("only1.txt"), b"only in first\n");
    let pid = fx.add_project();
    let s1 = fx.snapshot(&pid, &fx.repo);
    std::fs::remove_file(fx.repo.join("only1.txt")).unwrap();
    let s2 = fx.snapshot(&pid, &fx.repo);
    let gc = fx
        .vault
        .delete_snapshots(std::slice::from_ref(&s1.id))
        .unwrap();
    assert_eq!(gc.snapshots_pruned, 1);
    assert!(gc.blobs_deleted >= 1, "blob unique to s1 must be collected");
    // s2 still fully verifiable – shared blob was kept.
    assert!(fx.vault.verify_snapshot(&s2.id, true).unwrap().ok);
    // Pinned snapshots cannot be deleted.
    fx.vault.set_pinned(&s2.id, true).unwrap();
    assert!(
        fx.vault
            .delete_snapshots(std::slice::from_ref(&s2.id))
            .is_err()
    );
}

#[test]
fn interrupted_snapshots_are_marked_after_restart() {
    let fx = Fixture::new();
    let pid = fx.add_project();
    fx.vault
        .store()
        .begin_snapshot(
            "0190aaaa-0000-7000-8000-000000000001",
            &pid,
            &wr_core::paths::display(&fx.repo),
            SnapshotTrigger::Auto,
            1,
        )
        .unwrap();
    let root = fx.vault.root().to_path_buf();
    drop(fx.vault.store());
    let reopened = wr_core::vault::Vault::open(&root, false).unwrap();
    let s = reopened
        .store()
        .get_snapshot("0190aaaa-0000-7000-8000-000000000001")
        .unwrap();
    assert_eq!(s.status, SnapshotStatus::Interrupted);
}

#[test]
fn file_history_tracks_versions() {
    let fx = Fixture::new();
    let pid = fx.add_project();
    for v in ["one\n", "two\n", "three\n"] {
        write(&fx.repo.join("src/lib.rs"), v.as_bytes());
        fx.snapshot(&pid, &fx.repo);
    }
    let h = fx.vault.file_history(&pid, "src/lib.rs").unwrap();
    assert_eq!(h.len(), 3);
    let newest = fx
        .vault
        .blob_content(h[0].file.wt_blob.as_ref().unwrap(), 100)
        .unwrap();
    assert_eq!(newest.text.as_deref(), Some("three\n"));
    // HEAD version is available from Git for diffing.
    let head = fx
        .vault
        .git_blob_content(&pid, h[0].file.head_oid.as_ref().unwrap(), 100)
        .unwrap();
    assert_eq!(head.text.as_deref(), Some("pub fn a() -> u32 { 1 }\n"));
    // Path filter on snapshot queries.
    let q = fx
        .vault
        .query_snapshots(&SnapshotQuery {
            project_id: pid.clone(),
            path_contains: Some("lib.rs".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(q.len(), 3);
}

#[test]
fn forensic_discovery_finds_lost_commit() {
    let fx = Fixture::new();
    write(&fx.repo.join("lost.txt"), b"only in a lost commit\n");
    git(&fx.repo, &["add", "lost.txt"]);
    git(&fx.repo, &["commit", "-q", "-m", "work that will be lost"]);
    let lost = git(&fx.repo, &["rev-parse", "HEAD"]).trim().to_string();
    // Fixture simulates an agent running a destructive reset.
    git(&fx.repo, &["reset", "-q", "--hard", "HEAD~1"]);
    let pid = fx.add_project();
    let cancel = AtomicBool::new(false);
    let report = fx
        .vault
        .forensic_scan(&pid, Some(&fx.repo), true, &cancel)
        .unwrap();
    let cand = report
        .candidates
        .iter()
        .find(|c| c.oid == lost)
        .expect("lost commit discovered via reflog");
    assert_eq!(cand.provenance, "gitReflog");
    let target = fx.base.join("forensic");
    let r = fx
        .vault
        .recover_git_object(&pid, cand, &target, &[])
        .unwrap();
    assert_eq!(r.status, "completed", "{r:#?}");
    let short = &lost[..12];
    assert_eq!(
        std::fs::read(target.join(format!("commit-{short}/lost.txt"))).unwrap(),
        b"only in a lost commit\n"
    );
}

#[test]
fn demo_vault_builds_story() {
    let dir = tempfile::tempdir().unwrap();
    let vault = wr_core::vault::Vault::open(&dir.path().join("demo"), true).unwrap();
    let p = vault.build_demo().unwrap();
    assert!(p.is_demo);
    let wts = vault.list_worktrees(&p.id).unwrap();
    assert_eq!(wts.len(), 4);
    let diag: Vec<_> = wts.iter().map(|w| w.diagnosis).collect();
    assert!(diag.contains(&WorktreeDiagnosis::PossiblyMoved), "{diag:?}");
    assert!(
        diag.iter().any(|d| matches!(
            d,
            WorktreeDiagnosis::Missing | WorktreeDiagnosis::PrunableCandidate
        )),
        "{diag:?}"
    );
    let risks = vault.list_risks(&p.id, false).unwrap();
    assert!(!risks.is_empty());
    // The production vault refuses to build a demo.
    let prod = wr_core::vault::Vault::open(&dir.path().join("prod"), false).unwrap();
    assert!(prod.build_demo().is_err());
}
