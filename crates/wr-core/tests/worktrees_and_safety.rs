mod common;

use common::*;
use std::sync::atomic::AtomicBool;
use wr_core::recovery::Variant;
use wr_core::types::*;

fn wt<'a>(list: &'a [Worktree], suffix: &str) -> &'a Worktree {
    list.iter()
        .find(|w| w.path.replace('\\', "/").ends_with(suffix))
        .unwrap_or_else(|| {
            panic!(
                "no worktree ending in {suffix}: {:?}",
                list.iter().map(|w| &w.path).collect::<Vec<_>>()
            )
        })
}

#[test]
fn inventory_detects_linked_worktrees_locks_and_detached() {
    let fx = Fixture::new();
    let a = fx.base.join("wt-a");
    let b = fx.base.join("wt b 空格");
    git(
        &fx.repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feature/a",
            a.to_str().unwrap(),
        ],
    );
    git(
        &fx.repo,
        &["worktree", "add", "-q", "--detach", b.to_str().unwrap()],
    );
    git(
        &fx.repo,
        &[
            "worktree",
            "lock",
            "--reason",
            "agent busy",
            a.to_str().unwrap(),
        ],
    );
    write(&a.join("feature.txt"), b"wip\n");
    // Registering from a linked worktree resolves to the same project.
    let p = fx.vault.add_project(&a, Some(false)).unwrap();
    assert!(p.root_path.replace('\\', "/").ends_with("/repo"));
    let list = fx.vault.list_worktrees(&p.id).unwrap();
    assert_eq!(list.len(), 3);
    let wa = wt(&list, "/wt-a");
    assert!(wa.locked);
    assert_eq!(wa.lock_reason.as_deref(), Some("agent busy"));
    assert_eq!(wa.branch.as_deref(), Some("feature/a"));
    assert_eq!(wa.dirty, Some(true));
    assert_eq!(wa.diagnosis, WorktreeDiagnosis::Healthy);
    let wb = wt(&list, "/wt b 空格");
    assert!(wb.detached);
    // Registering the main repo again is rejected as a duplicate.
    assert!(fx.vault.add_project(&fx.repo, None).is_err());
}

#[test]
fn missing_and_moved_worktrees_are_diagnosed_with_evidence() {
    let fx = Fixture::new();
    let gone = fx.base.join("wt-gone");
    let moving = fx.base.join("wt-moving");
    git(
        &fx.repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "gone",
            gone.to_str().unwrap(),
        ],
    );
    git(
        &fx.repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "moving",
            moving.to_str().unwrap(),
        ],
    );
    write(&gone.join("unsaved.txt"), b"agent output never committed\n");
    write(&moving.join("note.txt"), b"moving\n");
    let p = fx.vault.add_project(&fx.repo, Some(false)).unwrap();
    // Snapshot only the worktree that will move.
    fx.snapshot(&p.id, &moving);

    std::fs::remove_dir_all(&gone).unwrap();
    std::fs::rename(&moving, fx.base.join("wt-moved-here")).unwrap();
    let r = fx.vault.refresh_project(&p.id).unwrap();
    let wg = wt(&r.worktrees, "/wt-gone");
    assert!(
        matches!(
            wg.diagnosis,
            WorktreeDiagnosis::PrunableCandidate | WorktreeDiagnosis::Missing
        ),
        "{:?}",
        wg.diagnosis
    );
    assert!(wg.evidence.contains(&"volumeMounted".to_string()));
    let wm = wt(&r.worktrees, "/wt-moving");
    assert_eq!(wm.diagnosis, WorktreeDiagnosis::PossiblyMoved);
    assert!(
        wm.possible_new_path
            .as_ref()
            .unwrap()
            .ends_with("wt-moved-here")
    );

    // Unsaved, never-snapshotted work in a vanished worktree is critical;
    // the moved one was protected, so it is only "attention".
    let risks = fx.vault.list_risks(&p.id, false).unwrap();
    let crit = risks
        .iter()
        .find(|r| r.rule == "unprotectedWorkUnreachable")
        .expect("critical risk");
    assert_eq!(crit.severity, Severity::Critical);
    assert!(crit.worktree_path.as_ref().unwrap().ends_with("wt-gone"));
    assert!(
        risks
            .iter()
            .any(|r| r.rule == "worktreeUnreachableProtected")
    );
    // Timeline recorded the disappearance.
    let events = fx
        .vault
        .query_events(&EventQuery {
            project_id: p.id.clone(),
            ..Default::default()
        })
        .unwrap();
    assert!(events.iter().any(|e| e.kind == "worktreeUnreachable"));

    // Moving it back resolves the risk and records the reappearance.
    std::fs::rename(fx.base.join("wt-moved-here"), &moving).unwrap();
    let r2 = fx.vault.refresh_project(&p.id).unwrap();
    assert_eq!(
        wt(&r2.worktrees, "/wt-moving").diagnosis,
        WorktreeDiagnosis::Healthy
    );
    assert!(
        r2.resolved_risks
            .iter()
            .any(|r| r.rule == "worktreeUnreachableProtected")
    );
}

#[test]
fn branch_switch_and_mass_deletion_events() {
    let fx = Fixture::new();
    let p = fx.vault.add_project(&fx.repo, Some(false)).unwrap();
    git(&fx.repo, &["switch", "-q", "-c", "topic"]);
    // Delete many tracked files at once.
    for i in 0..30 {
        write(&fx.repo.join(format!("many/f{i}.txt")), b"x\n");
    }
    git(&fx.repo, &["add", "-A"]);
    git(&fx.repo, &["commit", "-q", "-m", "many"]);
    std::fs::remove_dir_all(fx.repo.join("many")).unwrap();
    let r = fx.vault.refresh_project(&p.id).unwrap();
    assert!(
        r.events.contains(&"branchChanged".to_string()),
        "{:?}",
        r.events
    );
    let risks = fx.vault.list_risks(&p.id, false).unwrap();
    let md = risks
        .iter()
        .find(|r| r.rule == "massDeletion")
        .expect("mass deletion risk");
    assert_eq!(md.evidence["deleted"], 30);
}

/// The central promise: monitoring, snapshotting, verification, forensic
/// scans and separate-directory recovery never change the repository —
/// not the working tree, not the index, not HEAD, refs, reflogs or stash.
#[test]
fn read_only_operations_leave_repository_byte_identical() {
    let fx = Fixture::new();
    let linked = fx.base.join("linked");
    git(
        &fx.repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "linked",
            linked.to_str().unwrap(),
        ],
    );
    write(&fx.repo.join("src/lib.rs"), b"dirty main\n");
    write(&fx.repo.join("README.md"), b"staged\n");
    git(&fx.repo, &["add", "README.md"]);
    write(&fx.repo.join("untracked.txt"), b"u\n");
    write(&linked.join("wip.txt"), b"linked wip\n");
    // A stash entry exists and must survive untouched.
    write(&fx.repo.join("CHANGELOG.md"), b"stashed change\n");
    git(&fx.repo, &["stash", "push", "-q", "--", "CHANGELOG.md"]);

    let before_main = fx.fingerprint(&fx.repo);
    let before_linked = fx.fingerprint(&linked);

    let p = fx.vault.add_project(&fx.repo, Some(true)).unwrap();
    fx.vault.refresh_project(&p.id).unwrap();
    let s1 = fx.snapshot(&p.id, &fx.repo);
    let s2 = fx.snapshot(&p.id, &linked);
    fx.vault.verify_snapshot(&s1.id, true).unwrap();
    fx.vault.verify_snapshot(&s2.id, true).unwrap();
    let cancel = AtomicBool::new(false);
    fx.vault
        .forensic_scan(&p.id, Some(&fx.repo), true, &cancel)
        .unwrap();
    fx.vault
        .recover_to_directory(&s1.id, &fx.base.join("out1"), None, true, true, &[])
        .unwrap();
    // A preview (without execution) must not write either.
    fx.vault
        .preview_restore(&s1.id, vec!["src/lib.rs".into()], Variant::WorkingTree)
        .unwrap();
    fx.vault.refresh_project(&p.id).unwrap();
    fx.vault.run_retention().unwrap();

    assert_eq!(
        before_main,
        fx.fingerprint(&fx.repo),
        "main worktree changed"
    );
    assert_eq!(
        before_linked,
        fx.fingerprint(&linked),
        "linked worktree changed"
    );
}

/// A failed recovery (corrupted vault) must leave the repository intact.
#[test]
fn failed_in_place_restore_does_not_damage_repository() {
    let fx = Fixture::new();
    write(&fx.repo.join("src/lib.rs"), b"snapshot version\n");
    let p = fx.vault.add_project(&fx.repo, Some(false)).unwrap();
    let s = fx.snapshot(&p.id, &fx.repo);
    write(&fx.repo.join("src/lib.rs"), b"current precious version\n");
    let preview = fx
        .vault
        .preview_restore(&s.id, vec!["src/lib.rs".into()], Variant::WorkingTree)
        .unwrap();
    // Corrupt the source blob after preview.
    let sha_ = preview.items[0].snapshot_sha256.clone().unwrap();
    std::fs::write(fx.vault.blobs().path_for(&sha_).unwrap(), b"garbage").unwrap();
    let before = fx.fingerprint(&fx.repo);
    let cancel = AtomicBool::new(false);
    let report = fx
        .vault
        .execute_restore(&preview.id, &cancel, &NoProgress)
        .unwrap();
    assert_eq!(report.status, "aborted");
    assert_eq!(before, fx.fingerprint(&fx.repo));
    assert_eq!(
        std::fs::read(fx.repo.join("src/lib.rs")).unwrap(),
        b"current precious version\n"
    );
}

#[test]
fn symlinks_are_recorded_not_followed() {
    let fx = Fixture::new();
    let outside = fx.base.join("outside-secret");
    std::fs::create_dir_all(&outside).unwrap();
    write(&outside.join("secret.txt"), b"must not be captured\n");
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_dir(&outside, fx.repo.join("link-out")).is_ok();
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(&outside, fx.repo.join("link-out")).is_ok();
    if !made {
        eprintln!("symlink creation not permitted on this machine; skipping");
        return;
    }
    let p = fx.vault.add_project(&fx.repo, Some(false)).unwrap();
    let s = fx.snapshot(&p.id, &fx.repo);
    let files = fx.vault.snapshot_detail(&s.id).unwrap().files;
    assert!(
        files.iter().all(|f| !f.path.contains("secret.txt")),
        "{files:?}"
    );
    if let Some(l) = files.iter().find(|f| f.path == "link-out") {
        assert_eq!(l.kind, "symlink");
    }
}
