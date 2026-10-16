//! IPC commands. Every command is a thin, typed wrapper around `wr-core`;
//! long-running work runs on the blocking pool with cancellation and
//! progress events.

use crate::state::{AppState, CmdResult, err};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use wr_core::diagnostics::{self, EnvironmentReport};
use wr_core::error::{CoreError, ErrorPayload};
use wr_core::forensics::{ForensicCandidate, ForensicReport};
use wr_core::recovery::{RecoveryReport, RestorePreview, Variant};
use wr_core::retention::GcReport;
use wr_core::settings::Settings;
use wr_core::snapshot::CaptureOutcome;
use wr_core::types::*;
use wr_core::vault::{RefreshResult, RepoProbe, SnapshotDetail, VerifyResult};

type S<'a> = State<'a, Arc<AppState>>;

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> CmdResult<T> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| ErrorPayload {
            code: "other".into(),
            message: e.to_string(),
        })?
}

pub struct EmitProgress(pub AppHandle);
impl ProgressSink for EmitProgress {
    fn report(&self, p: Progress) {
        let _ = self.0.emit("wr://progress", p);
    }
}

pub fn notify_changed(app: &AppHandle, project_id: &str, scope: &str) {
    let _ = app.emit(
        "wr://changed",
        json!({ "projectId": project_id, "scope": scope }),
    );
}

// ---------------------------------------------------------------------------
// Environment & settings
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub environment: EnvironmentReport,
    pub onboarded: bool,
    pub demo_data_dir: String,
    pub log_path: String,
    pub privacy: Vec<&'static str>,
}

#[tauri::command]
pub async fn app_info(state: S<'_>) -> CmdResult<AppInfo> {
    let st = state.inner().clone();
    blocking(move || {
        let s = st.main.settings();
        let git = st.main.git_opt();
        let environment = diagnostics::inspect(git.as_ref(), &st.data_dir, &s.recovery_dir);
        let onboarded = st.main.store().get_kv("onboarded").map_err(err)?.is_some();
        Ok(AppInfo {
            environment,
            onboarded,
            demo_data_dir: wr_core::paths::display(st.demo.root()),
            log_path: wr_core::paths::display(&st.log_path),
            privacy: vec!["offline", "noTelemetry", "noUpload", "noAccount"],
        })
    })
    .await
}

#[tauri::command]
pub async fn complete_onboarding(state: S<'_>) -> CmdResult<()> {
    state
        .main
        .store()
        .set_kv("onboarded", &wr_core::now_ms().to_string())
        .map_err(err)
}

#[tauri::command]
pub async fn get_settings(state: S<'_>) -> CmdResult<Settings> {
    Ok(state.main.settings())
}

#[tauri::command]
pub async fn update_settings(state: S<'_>, settings: Settings) -> CmdResult<Settings> {
    let saved = state.main.update_settings(settings).map_err(err)?;
    state.demo.mirror_settings(&saved);
    Ok(saved)
}

#[tauri::command]
pub async fn default_settings() -> CmdResult<Settings> {
    Ok(Settings::default())
}

// ---------------------------------------------------------------------------
// Projects & worktrees
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn probe_repository(state: S<'_>, path: String) -> CmdResult<RepoProbe> {
    let st = state.inner().clone();
    blocking(move || st.main.probe_repository(Path::new(&path)).map_err(err)).await
}

#[tauri::command]
pub async fn add_project(
    app: AppHandle,
    state: S<'_>,
    path: String,
    auto_protect: bool,
) -> CmdResult<Project> {
    let st = state.inner().clone();
    let p = blocking(move || {
        st.main
            .add_project(Path::new(&path), Some(auto_protect))
            .map_err(err)
    })
    .await?;
    notify_changed(&app, &p.id, "projects");
    Ok(p)
}

#[tauri::command]
pub async fn list_projects(state: S<'_>) -> CmdResult<Vec<Project>> {
    let st = state.inner().clone();
    blocking(move || {
        let mut all = st.main.list_projects().map_err(err)?;
        all.extend(st.demo.list_projects().map_err(err)?);
        Ok(all)
    })
    .await
}

#[tauri::command]
pub async fn remove_project(
    app: AppHandle,
    state: S<'_>,
    project_id: String,
    delete_backups: bool,
) -> CmdResult<GcReport> {
    let v = state.vault_for(&project_id)?;
    let id = project_id.clone();
    let r = blocking(move || v.remove_project(&id, delete_backups).map_err(err)).await?;
    notify_changed(&app, &project_id, "projects");
    Ok(r)
}

#[tauri::command]
pub async fn set_auto_protect(
    app: AppHandle,
    state: S<'_>,
    project_id: String,
    enabled: bool,
) -> CmdResult<()> {
    let v = state.vault_for(&project_id)?;
    v.set_auto_protect(&project_id, enabled).map_err(err)?;
    if enabled {
        v.set_monitoring(&project_id, Monitoring::Active)
            .map_err(err)?;
    }
    notify_changed(&app, &project_id, "projects");
    Ok(())
}

#[tauri::command]
pub async fn set_monitoring(
    app: AppHandle,
    state: S<'_>,
    project_id: String,
    active: bool,
) -> CmdResult<()> {
    let v = state.vault_for(&project_id)?;
    v.set_monitoring(
        &project_id,
        if active {
            Monitoring::Active
        } else {
            Monitoring::Paused
        },
    )
    .map_err(err)?;
    notify_changed(&app, &project_id, "projects");
    Ok(())
}

#[tauri::command]
pub async fn refresh_project(
    app: AppHandle,
    state: S<'_>,
    project_id: String,
) -> CmdResult<RefreshResult> {
    let v = state.vault_for(&project_id)?;
    let id = project_id.clone();
    let r = blocking(move || v.refresh_project(&id).map_err(err)).await?;
    crate::monitor::announce_risks(&app, &state, &r.new_risks);
    notify_changed(&app, &project_id, "inventory");
    Ok(r)
}

#[tauri::command]
pub async fn list_worktrees(state: S<'_>, project_id: String) -> CmdResult<Vec<Worktree>> {
    let v = state.vault_for(&project_id)?;
    blocking(move || v.list_worktrees(&project_id).map_err(err)).await
}

#[tauri::command]
pub async fn worktree_guidance(
    state: S<'_>,
    project_id: String,
    worktree_path: String,
) -> CmdResult<Vec<String>> {
    let v = state.vault_for(&project_id)?;
    let list = v.list_worktrees(&project_id).map_err(err)?;
    let w = list
        .iter()
        .find(|w| wr_core::paths::same_path(Path::new(&w.path), Path::new(&worktree_path)))
        .ok_or_else(|| err(CoreError::InvalidTarget(worktree_path.clone())))?;
    Ok(wr_core::worktree::repair_guidance(w))
}

// ---------------------------------------------------------------------------
// Snapshots
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn create_snapshot(
    app: AppHandle,
    state: S<'_>,
    project_id: String,
    worktree_path: String,
    note: Option<String>,
    task_id: String,
) -> CmdResult<CaptureOutcome> {
    let v = state.vault_for(&project_id)?;
    let st = state.inner().clone();
    let flag = st.register_task(&task_id);
    let app2 = app.clone();
    let tid = task_id.clone();
    let pid = project_id.clone();
    let res = blocking(move || {
        let sink = EmitProgress(app2);
        v.create_snapshot(
            &pid,
            Path::new(&worktree_path),
            SnapshotTrigger::Manual,
            note.filter(|n| !n.trim().is_empty()),
            &flag,
            &sink,
            &tid,
        )
        .map_err(err)
    })
    .await;
    st.finish_task(&task_id);
    notify_changed(&app, &project_id, "snapshots");
    res
}

#[tauri::command]
pub async fn cancel_task(state: S<'_>, task_id: String) -> CmdResult<bool> {
    if let Some(flag) = state.tasks.lock().unwrap().get(&task_id) {
        flag.store(true, std::sync::atomic::Ordering::Relaxed);
        return Ok(true);
    }
    Ok(false)
}

#[tauri::command]
pub async fn query_snapshots(
    state: S<'_>,
    query: SnapshotQuery,
) -> CmdResult<Vec<SnapshotSummary>> {
    let v = state.vault_for(&query.project_id)?;
    blocking(move || v.query_snapshots(&query).map_err(err)).await
}

#[tauri::command]
pub async fn snapshot_detail(state: S<'_>, snapshot_id: String) -> CmdResult<SnapshotDetail> {
    let v = state.vault_for_snapshot(&snapshot_id)?;
    blocking(move || v.snapshot_detail(&snapshot_id).map_err(err)).await
}

#[tauri::command]
pub async fn verify_snapshot(
    app: AppHandle,
    state: S<'_>,
    snapshot_id: String,
    deep: bool,
) -> CmdResult<VerifyResult> {
    let v = state.vault_for_snapshot(&snapshot_id)?;
    let sid = snapshot_id.clone();
    let r = blocking(move || v.verify_snapshot(&sid, deep).map_err(err)).await?;
    let pid = state
        .vault_for_snapshot(&snapshot_id)?
        .store()
        .get_snapshot(&snapshot_id)
        .map(|s| s.project_id)
        .unwrap_or_default();
    notify_changed(&app, &pid, "snapshots");
    Ok(r)
}

#[tauri::command]
pub async fn set_snapshot_pinned(
    app: AppHandle,
    state: S<'_>,
    snapshot_id: String,
    pinned: bool,
) -> CmdResult<()> {
    let v = state.vault_for_snapshot(&snapshot_id)?;
    v.set_pinned(&snapshot_id, pinned).map_err(err)?;
    let pid = v
        .store()
        .get_snapshot(&snapshot_id)
        .map(|s| s.project_id)
        .unwrap_or_default();
    notify_changed(&app, &pid, "snapshots");
    Ok(())
}

#[tauri::command]
pub async fn set_snapshot_note(
    state: S<'_>,
    snapshot_id: String,
    note: Option<String>,
) -> CmdResult<()> {
    let v = state.vault_for_snapshot(&snapshot_id)?;
    v.set_note(
        &snapshot_id,
        note.as_deref().filter(|n| !n.trim().is_empty()),
    )
    .map_err(err)
}

#[tauri::command]
pub async fn delete_snapshots(
    app: AppHandle,
    state: S<'_>,
    project_id: String,
    snapshot_ids: Vec<String>,
) -> CmdResult<GcReport> {
    let v = state.vault_for(&project_id)?;
    let r = blocking(move || v.delete_snapshots(&snapshot_ids).map_err(err)).await?;
    notify_changed(&app, &project_id, "snapshots");
    Ok(r)
}

#[tauri::command]
pub async fn file_history(
    state: S<'_>,
    project_id: String,
    path: String,
) -> CmdResult<Vec<FileVersion>> {
    let v = state.vault_for(&project_id)?;
    blocking(move || v.file_history(&project_id, &path).map_err(err)).await
}

const PREVIEW_LIMIT: usize = 2 * 1024 * 1024;

#[tauri::command]
pub async fn blob_content(
    state: S<'_>,
    project_id: String,
    sha256: String,
) -> CmdResult<BlobContent> {
    let v = state.vault_for(&project_id)?;
    blocking(move || v.blob_content(&sha256, PREVIEW_LIMIT).map_err(err)).await
}

#[tauri::command]
pub async fn git_blob_content(
    state: S<'_>,
    project_id: String,
    oid: String,
) -> CmdResult<BlobContent> {
    let v = state.vault_for(&project_id)?;
    blocking(move || {
        v.git_blob_content(&project_id, &oid, PREVIEW_LIMIT)
            .map_err(err)
    })
    .await
}

#[tauri::command]
pub async fn current_file_content(
    state: S<'_>,
    project_id: String,
    worktree_path: String,
    path: String,
) -> CmdResult<BlobContent> {
    let v = state.vault_for(&project_id)?;
    blocking(move || {
        v.current_file_content(&project_id, Path::new(&worktree_path), &path, PREVIEW_LIMIT)
            .map_err(err)
    })
    .await
}

// ---------------------------------------------------------------------------
// Recovery
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn suggest_recovery_target(state: S<'_>, snapshot_id: String) -> CmdResult<String> {
    let v = state.vault_for_snapshot(&snapshot_id)?;
    let s = v.store().get_snapshot(&snapshot_id).map_err(err)?;
    let p = v.store().get_project(&s.project_id).map_err(err)?;
    let settings = state.main.settings();
    let stamp = chrono_stamp(s.created_at);
    let safe_name: String = p
        .name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let mut target = PathBuf::from(&settings.recovery_dir)
        .join(format!("{safe_name}-{stamp}-{}", &s.id[s.id.len() - 6..]));
    let mut n = 2;
    while target.exists() {
        target = PathBuf::from(&settings.recovery_dir).join(format!(
            "{safe_name}-{stamp}-{}-{n}",
            &s.id[s.id.len() - 6..]
        ));
        n += 1;
    }
    Ok(wr_core::paths::display(&target))
}

fn chrono_stamp(ms: i64) -> String {
    // yyyymmdd-hhmmss in UTC without pulling chrono into the shell crate.
    let secs = ms / 1000;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}{mo:02}{d:02}-{h:02}{m:02}{s:02}")
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryRecoveryRequest {
    pub snapshot_id: String,
    pub target: String,
    pub selection: Option<Vec<String>>,
    pub include_staged: bool,
    pub include_head_files: bool,
}

#[tauri::command]
pub async fn recover_to_directory(
    app: AppHandle,
    state: S<'_>,
    request: DirectoryRecoveryRequest,
) -> CmdResult<RecoveryReport> {
    let v = state.vault_for_snapshot(&request.snapshot_id)?;
    // The demo vault's sandbox and the main vault are both protected.
    let extra = vec![
        state.main.root().to_path_buf(),
        state.demo.root().to_path_buf(),
    ];
    let pid = v
        .store()
        .get_snapshot(&request.snapshot_id)
        .map(|s| s.project_id)
        .unwrap_or_default();
    let r = blocking(move || {
        v.recover_to_directory(
            &request.snapshot_id,
            Path::new(&request.target),
            request.selection,
            request.include_staged,
            request.include_head_files,
            &extra,
        )
        .map_err(err)
    })
    .await?;
    notify_changed(&app, &pid, "recoveries");
    Ok(r)
}

#[tauri::command]
pub async fn preview_restore(
    state: S<'_>,
    snapshot_id: String,
    paths: Vec<String>,
    variant: Variant,
) -> CmdResult<RestorePreview> {
    let v = state.vault_for_snapshot(&snapshot_id)?;
    blocking(move || v.preview_restore(&snapshot_id, paths, variant).map_err(err)).await
}

#[tauri::command]
pub async fn execute_restore(
    app: AppHandle,
    state: S<'_>,
    project_id: String,
    preview_id: String,
    task_id: String,
) -> CmdResult<RecoveryReport> {
    let v = state.vault_for(&project_id)?;
    let st = state.inner().clone();
    let flag = st.register_task(&task_id);
    let app2 = app.clone();
    let r = blocking(move || {
        let sink = EmitProgress(app2);
        v.execute_restore(&preview_id, &flag, &sink).map_err(err)
    })
    .await;
    st.finish_task(&task_id);
    notify_changed(&app, &project_id, "recoveries");
    r
}

#[tauri::command]
pub async fn list_recoveries(
    state: S<'_>,
    project_id: String,
) -> CmdResult<Vec<serde_json::Value>> {
    let v = state.vault_for(&project_id)?;
    blocking(move || v.list_recoveries(&project_id).map_err(err)).await
}

// ---------------------------------------------------------------------------
// Forensics
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn forensic_scan(
    state: S<'_>,
    project_id: String,
    worktree_path: Option<String>,
    deep: bool,
    task_id: String,
) -> CmdResult<ForensicReport> {
    let v = state.vault_for(&project_id)?;
    let st = state.inner().clone();
    let flag = st.register_task(&task_id);
    let r = blocking(move || {
        v.forensic_scan(
            &project_id,
            worktree_path.as_deref().map(Path::new),
            deep,
            &flag,
        )
        .map_err(err)
    })
    .await;
    st.finish_task(&task_id);
    r
}

#[tauri::command]
pub async fn forensic_commit_files(
    state: S<'_>,
    project_id: String,
    oid: String,
) -> CmdResult<Vec<(String, String)>> {
    let v = state.vault_for(&project_id)?;
    blocking(move || v.forensic_commit_files(&project_id, &oid).map_err(err)).await
}

#[tauri::command]
pub async fn recover_git_object(
    app: AppHandle,
    state: S<'_>,
    project_id: String,
    candidate: ForensicCandidate,
    target: String,
) -> CmdResult<RecoveryReport> {
    let v = state.vault_for(&project_id)?;
    let extra = vec![
        state.main.root().to_path_buf(),
        state.demo.root().to_path_buf(),
    ];
    let pid = project_id.clone();
    let r = blocking(move || {
        v.recover_git_object(&pid, &candidate, Path::new(&target), &extra)
            .map_err(err)
    })
    .await?;
    notify_changed(&app, &project_id, "recoveries");
    Ok(r)
}

#[tauri::command]
pub async fn suggest_forensic_target(
    state: S<'_>,
    project_id: String,
    oid: String,
) -> CmdResult<String> {
    let v = state.vault_for(&project_id)?;
    let p = v.store().get_project(&project_id).map_err(err)?;
    let settings = state.main.settings();
    let short: String = oid.chars().take(10).collect();
    let mut target = PathBuf::from(&settings.recovery_dir).join(format!("{}-git-{short}", p.name));
    let mut n = 2;
    while target.exists() {
        target = PathBuf::from(&settings.recovery_dir).join(format!("{}-git-{short}-{n}", p.name));
        n += 1;
    }
    Ok(wr_core::paths::display(&target))
}

// ---------------------------------------------------------------------------
// Risks & timeline
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_risks(
    state: S<'_>,
    project_id: String,
    include_resolved: bool,
) -> CmdResult<Vec<Risk>> {
    let v = state.vault_for(&project_id)?;
    blocking(move || v.list_risks(&project_id, include_resolved).map_err(err)).await
}

#[tauri::command]
pub async fn acknowledge_risk(
    app: AppHandle,
    state: S<'_>,
    project_id: String,
    risk_id: String,
) -> CmdResult<()> {
    let v = state.vault_for(&project_id)?;
    v.acknowledge_risk(&risk_id).map_err(err)?;
    notify_changed(&app, &project_id, "risks");
    Ok(())
}

#[tauri::command]
pub async fn query_events(state: S<'_>, query: EventQuery) -> CmdResult<Vec<TimelineEvent>> {
    let v = state.vault_for(&query.project_id)?;
    blocking(move || v.query_events(&query).map_err(err)).await
}

// ---------------------------------------------------------------------------
// Storage, demo, diagnostics
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageOverview {
    pub main: StorageStats,
    pub demo: StorageStats,
}

#[tauri::command]
pub async fn storage_stats(state: S<'_>) -> CmdResult<StorageOverview> {
    let st = state.inner().clone();
    blocking(move || {
        Ok(StorageOverview {
            main: st.main.storage_stats().map_err(err)?,
            demo: st.demo.storage_stats().map_err(err)?,
        })
    })
    .await
}

#[tauri::command]
pub async fn retention_plan(state: S<'_>, project_id: String) -> CmdResult<Vec<String>> {
    let v = state.vault_for(&project_id)?;
    blocking(move || v.retention_plan(&project_id).map_err(err)).await
}

#[tauri::command]
pub async fn run_retention(app: AppHandle, state: S<'_>) -> CmdResult<GcReport> {
    let st = state.inner().clone();
    let r = blocking(move || {
        let mut total = st.main.run_retention().map_err(err)?;
        let d = st.demo.run_retention().map_err(err)?;
        total.snapshots_pruned += d.snapshots_pruned;
        total.blobs_deleted += d.blobs_deleted;
        total.bytes_freed += d.bytes_freed;
        total.orphan_files_deleted += d.orphan_files_deleted;
        Ok(total)
    })
    .await?;
    notify_changed(&app, "", "storage");
    Ok(r)
}

#[tauri::command]
pub async fn build_demo(app: AppHandle, state: S<'_>) -> CmdResult<Project> {
    let st = state.inner().clone();
    let p = blocking(move || st.demo.build_demo().map_err(err)).await?;
    notify_changed(&app, &p.id, "projects");
    Ok(p)
}

#[tauri::command]
pub async fn export_diagnostics(state: S<'_>, target: String) -> CmdResult<String> {
    let st = state.inner().clone();
    blocking(move || {
        let settings = st.main.settings();
        let env = diagnostics::inspect(st.main.git_opt().as_ref(), &st.data_dir, &settings.recovery_dir);
        let mut projects = Vec::new();
        for v in st.vaults() {
            for p in v.list_projects().map_err(err)? {
                let worktrees = v.list_worktrees(&p.id).map_err(err)?;
                let risks = v.list_risks(&p.id, false).map_err(err)?;
                projects.push(json!({
                    "project": p,
                    "worktrees": worktrees,
                    "openRisks": risks,
                }));
            }
        }
        let report = json!({
            "generatedAt": wr_core::now_ms(),
            "note": "Contains paths, settings and status metadata only. No file contents are included.",
            "environment": env,
            "settings": settings,
            "storage": { "main": st.main.storage_stats().ok(), "demo": st.demo.storage_stats().ok() },
            "schemaVersion": st.main.store().schema_version().ok(),
            "projects": projects,
            "logTail": diagnostics::tail(&st.log_path, 400),
        });
        let path = PathBuf::from(&target);
        std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap_or_default())
            .map_err(|e| err(CoreError::io(&path, e)))?;
        Ok(wr_core::paths::display(&path))
    })
    .await
}

#[tauri::command]
pub async fn reveal_path(path: String) -> CmdResult<()> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(err(CoreError::InvalidTarget(format!(
            "{path} does not exist"
        ))));
    }
    tauri_plugin_opener::reveal_item_in_dir(&p).map_err(|e| ErrorPayload {
        code: "other".into(),
        message: e.to_string(),
    })
}
