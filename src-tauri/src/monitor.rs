//! Background protection services:
//!
//! - a file-system watcher per protected worktree (auto protection only),
//! - a debounced scheduler: snapshot after `quietPeriod` of silence, never
//!   more often than `minInterval`, with a forced checkpoint every
//!   `checkpointInterval` during continuous editing,
//! - a periodic inventory/risk pass for every monitored project.
//!
//! The watcher only *observes*; snapshots read the worktree through the
//! same read-only engine as manual captures.

use crate::commands::{EmitProgress, notify_changed};
use crate::state::AppState;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;
use wr_core::store::path_key;
use wr_core::types::*;
use wr_core::vault::Vault;

#[derive(Clone)]
struct Target {
    project_id: String,
    worktree: PathBuf,
    vault: Arc<Vault>,
}

#[derive(Default)]
struct Pending {
    first_change: i64,
    last_change: i64,
}

#[derive(Default)]
struct Shared {
    pending: HashMap<String, Pending>,
    last_attempt: HashMap<String, i64>,
    /// Last change time already written to the database per worktree.
    noted: HashMap<String, i64>,
}

/// Paths whose changes never trigger snapshots (Git internals and caches).
fn is_noise(rel: &str) -> bool {
    let r = rel.replace('\\', "/");
    let first = r.split('/').next().unwrap_or("");
    if first == ".git" {
        // HEAD/index changes signal branch switches or staging: keep those.
        return !(r == ".git/HEAD" || r == ".git/index");
    }
    r.split('/').any(|c| {
        matches!(
            c,
            "node_modules"
                | "target"
                | ".next"
                | ".turbo"
                | "__pycache__"
                | ".venv"
                | ".cache"
                | "dist"
                | "build"
        )
    }) || r.ends_with(".wr-restore.part")
        || r.contains("/.wr-restore-")
}

pub fn start(app: AppHandle, state: Arc<AppState>) {
    let shared = Arc::new(Mutex::new(Shared::default()));
    let watchers: Arc<Mutex<HashMap<String, RecommendedWatcher>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let targets: Arc<Mutex<HashMap<String, Target>>> = Arc::new(Mutex::new(HashMap::new()));

    // Inventory / reconciliation loop.
    {
        let app = app.clone();
        let state = state.clone();
        let shared = shared.clone();
        let watchers = watchers.clone();
        let targets = targets.clone();
        std::thread::Builder::new()
            .name("wr-inventory".into())
            .spawn(move || {
                let mut first = true;
                loop {
                    inventory_pass(&app, &state, first);
                    reconcile(&state, &shared, &watchers, &targets);
                    first = false;
                    let secs = state.main.settings().inventory_poll_secs.clamp(15, 3600) as u64;
                    std::thread::sleep(Duration::from_secs(secs));
                }
            })
            .expect("spawn inventory thread");
    }

    // Scheduler loop.
    std::thread::Builder::new()
        .name("wr-scheduler".into())
        .spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(2));
                schedule_tick(&app, &state, &shared, &targets);
            }
        })
        .expect("spawn scheduler thread");
}

fn inventory_pass(app: &AppHandle, state: &Arc<AppState>, first: bool) {
    for vault in state.vaults() {
        let Ok(projects) = vault.list_projects() else {
            continue;
        };
        for p in projects {
            if p.monitoring != Monitoring::Active && !first {
                continue;
            }
            match vault.refresh_project(&p.id) {
                Ok(r) => {
                    announce_risks(app, state, &r.new_risks);
                    if !r.events.is_empty()
                        || !r.new_risks.is_empty()
                        || !r.resolved_risks.is_empty()
                    {
                        notify_changed(app, &p.id, "inventory");
                    }
                }
                Err(e) => log::warn!("inventory failed for {}: {e}", p.name),
            }
        }
    }
    // Daily retention.
    let now = wr_core::now_ms();
    let last: i64 = state
        .main
        .store()
        .get_kv("lastRetention")
        .ok()
        .flatten()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if now - last > 24 * 3_600_000 {
        for v in state.vaults() {
            if let Err(e) = v.run_retention() {
                log::warn!("retention failed: {e}");
            }
        }
        let _ = state.main.store().set_kv("lastRetention", &now.to_string());
        notify_changed(app, "", "storage");
    }
}

/// Keep exactly one watcher per healthy worktree of auto-protected,
/// actively monitored projects.
fn reconcile(
    state: &Arc<AppState>,
    shared: &Arc<Mutex<Shared>>,
    watchers: &Arc<Mutex<HashMap<String, RecommendedWatcher>>>,
    targets: &Arc<Mutex<HashMap<String, Target>>>,
) {
    let mut desired: HashMap<String, Target> = HashMap::new();
    for vault in state.vaults() {
        let Ok(projects) = vault.list_projects() else {
            continue;
        };
        for p in projects {
            if !p.auto_protect || p.monitoring != Monitoring::Active {
                continue;
            }
            let Ok(wts) = vault.list_worktrees(&p.id) else {
                continue;
            };
            for w in wts {
                if w.diagnosis == WorktreeDiagnosis::Healthy && !w.gone_from_git {
                    desired.insert(
                        path_key(&w.path),
                        Target {
                            project_id: p.id.clone(),
                            worktree: PathBuf::from(&w.path),
                            vault: vault.clone(),
                        },
                    );
                }
            }
        }
    }
    let mut ws = watchers.lock().unwrap();
    let existing: HashSet<String> = ws.keys().cloned().collect();
    for key in existing.difference(&desired.keys().cloned().collect()) {
        ws.remove(key);
        shared.lock().unwrap().pending.remove(key);
    }
    for (key, t) in &desired {
        if ws.contains_key(key) {
            continue;
        }
        let shared_cb = shared.clone();
        let root = t.worktree.clone();
        let k = key.clone();
        let handler = move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else { return };
            if matches!(event.kind, notify::EventKind::Access(_)) {
                return;
            }
            let relevant = event.paths.iter().any(|p| {
                p.strip_prefix(&root)
                    .map(|rel| !is_noise(&rel.to_string_lossy()))
                    .unwrap_or(false)
            });
            if !relevant {
                return;
            }
            let now = wr_core::now_ms();
            let mut s = shared_cb.lock().unwrap();
            let entry = s.pending.entry(k.clone()).or_default();
            if entry.first_change == 0 {
                entry.first_change = now;
            }
            entry.last_change = now;
        };
        match notify::recommended_watcher(handler) {
            Ok(mut w) => {
                if let Err(e) = w.watch(&t.worktree, RecursiveMode::Recursive) {
                    log::warn!("cannot watch {}: {e}", t.worktree.display());
                    continue;
                }
                ws.insert(key.clone(), w);
                // Changes made while the app was closed: check once.
                let now = wr_core::now_ms();
                shared
                    .lock()
                    .unwrap()
                    .pending
                    .entry(key.clone())
                    .or_insert(Pending {
                        first_change: now,
                        last_change: now - 1_000_000,
                    });
            }
            Err(e) => log::warn!("watcher error: {e}"),
        }
    }
    *targets.lock().unwrap() = desired;
}

fn schedule_tick(
    app: &AppHandle,
    state: &Arc<AppState>,
    shared: &Arc<Mutex<Shared>>,
    targets: &Arc<Mutex<HashMap<String, Target>>>,
) {
    let settings = state.main.settings();
    let now = wr_core::now_ms();
    let quiet = settings.quiet_period_secs * 1000;
    let min_interval = settings.min_interval_secs * 1000;
    let checkpoint = settings.checkpoint_interval_secs * 1000;
    // Persist watcher observations so risk rules know about uncaptured edits,
    // and react at once when a watched worktree directory disappears.
    let observed: Vec<(String, i64)> = {
        let s = shared.lock().unwrap();
        s.pending
            .iter()
            .filter(|(k, p)| {
                p.last_change > 0 && s.noted.get(*k).copied().unwrap_or(0) < p.last_change
            })
            .map(|(k, p)| (k.clone(), p.last_change))
            .collect()
    };
    for (key, at) in observed {
        let Some(t) = targets.lock().unwrap().get(&key).cloned() else {
            continue;
        };
        shared.lock().unwrap().noted.insert(key.clone(), at);
        if !t.worktree.exists() {
            shared.lock().unwrap().pending.remove(&key);
            if let Ok(r) = t.vault.refresh_project(&t.project_id) {
                announce_risks(app, state, &r.new_risks);
            }
            notify_changed(app, &t.project_id, "inventory");
            continue;
        }
        if at > now - 1_000_000 {
            let _ = t.vault.note_change(&t.project_id, &t.worktree, at);
        }
    }
    let due: Vec<(String, SnapshotTrigger)> = {
        let s = shared.lock().unwrap();
        s.pending
            .iter()
            .filter_map(|(k, p)| {
                let last_attempt = s.last_attempt.get(k).copied().unwrap_or(0);
                if now - last_attempt < min_interval {
                    return None;
                }
                if now - p.last_change >= quiet {
                    Some((k.clone(), SnapshotTrigger::Auto))
                } else if now - p.first_change >= checkpoint {
                    Some((k.clone(), SnapshotTrigger::Checkpoint))
                } else {
                    None
                }
            })
            .collect()
    };
    for (key, trigger) in due {
        let Some(t) = targets.lock().unwrap().get(&key).cloned() else {
            shared.lock().unwrap().pending.remove(&key);
            continue;
        };
        {
            let mut s = shared.lock().unwrap();
            s.pending.remove(&key);
            s.last_attempt.insert(key.clone(), now);
        }
        let cancel = AtomicBool::new(false);
        let sink = EmitProgress(app.clone());
        let task_id = format!("auto-{}", uuid::Uuid::new_v4().simple());
        match t.vault.create_snapshot(
            &t.project_id,
            &t.worktree,
            trigger,
            None,
            &cancel,
            &sink,
            &task_id,
        ) {
            Ok(out) => {
                notify_changed(
                    app,
                    &t.project_id,
                    if out.snapshot.is_some() {
                        "snapshots"
                    } else {
                        "inventory"
                    },
                );
            }
            Err(wr_core::CoreError::Busy) => {
                // Retry on the next tick.
                let mut s = shared.lock().unwrap();
                s.pending.insert(
                    key,
                    Pending {
                        first_change: now,
                        last_change: now - quiet,
                    },
                );
            }
            Err(e) => {
                log::warn!("auto snapshot failed for {}: {e}", t.worktree.display());
                notify_changed(app, &t.project_id, "snapshots");
                if settings.notify_on_failure {
                    let _ = app
                        .notification()
                        .builder()
                        .title("Worktree Rescue")
                        .body(format!(
                            "Automatic snapshot failed: {}",
                            short_path(&t.worktree)
                        ))
                        .show();
                }
            }
        }
    }
}

fn short_path(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}

/// Desktop notification for newly raised attention/critical risks.
pub fn announce_risks(app: &AppHandle, state: &Arc<AppState>, risks: &[Risk]) {
    if !state.main.settings().notify_on_risk {
        return;
    }
    for r in risks.iter().filter(|r| r.severity != Severity::Info) {
        let where_ = r
            .worktree_path
            .as_deref()
            .map(|p| short_path(Path::new(p)))
            .unwrap_or_default();
        let title = match r.severity {
            Severity::Critical => "Critical: work may be at risk",
            _ => "Attention required",
        };
        let body = match r.rule.as_str() {
            "unprotectedWorkUnreachable" => {
                format!("{where_}: worktree unreachable with work that was never snapshotted")
            }
            "worktreeUnreachableProtected" => {
                format!("{where_}: worktree unreachable — latest work is in a snapshot")
            }
            "massDeletion" => format!("{where_}: many tracked files were deleted"),
            "autoProtectFailing" => format!("{where_}: automatic snapshots keep failing"),
            "storageLow" => "Snapshot storage is running out of disk space".to_string(),
            "budgetNearlyFull" => "A repository is close to its storage budget".to_string(),
            "staleProtection" => format!("{where_}: changes have not been snapshotted for a while"),
            _ => format!("{where_}: {}", r.rule),
        };
        let _ = app.notification().builder().title(title).body(body).show();
    }
    let _ = app.get_webview_window("main");
}

#[cfg(test)]
mod tests {
    use super::is_noise;

    #[test]
    fn noise_filter() {
        assert!(is_noise(".git/objects/ab/cd"));
        assert!(!is_noise(".git/HEAD"));
        assert!(!is_noise(".git/index"));
        assert!(is_noise("web/node_modules/x.js"));
        assert!(!is_noise("src/main.rs"));
    }
}
