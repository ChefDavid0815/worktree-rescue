//! Worktree Rescue desktop shell (Tauri 2).

mod commands;
mod monitor;
mod state;

use state::AppState;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::Manager;
use wr_core::vault::Vault;

/// Data root. `WR_DATA_DIR` overrides it (used for isolated test runs).
fn data_dir() -> PathBuf {
    std::env::var_os("WR_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(wr_core::platform::default_data_dir)
}

fn init_logging(dir: &std::path::Path) -> PathBuf {
    let logs = dir.join("logs");
    let _ = std::fs::create_dir_all(&logs);
    let path = logs.join("app.log");
    // Simple rotation: keep one previous file once the log exceeds 5 MB.
    if std::fs::metadata(&path)
        .map(|m| m.len() > 5 * 1024 * 1024)
        .unwrap_or(false)
    {
        let _ = std::fs::rename(&path, logs.join("app.log.1"));
    }
    if let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let config = simplelog::ConfigBuilder::new()
            .add_filter_allow_str("wr_core")
            .add_filter_allow_str("worktree_rescue_lib")
            .build();
        let _ = simplelog::WriteLogger::init(log::LevelFilter::Info, config, file);
    }
    path
}

pub fn run() {
    let dir = data_dir();
    let log_path = init_logging(&dir);
    log::info!(
        "Worktree Rescue {} starting; data dir {}",
        env!("CARGO_PKG_VERSION"),
        dir.display()
    );

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(move |app| {
            let main = Arc::new(Vault::open(&dir.join("vault"), false)?);
            let demo = Arc::new(Vault::open(&dir.join("demo-vault"), true)?);
            demo.mirror_settings(&main.settings());
            let state = Arc::new(AppState {
                main,
                demo,
                data_dir: dir.clone(),
                log_path: log_path.clone(),
                tasks: Mutex::new(HashMap::new()),
            });
            app.manage(state.clone());
            monitor::start(app.handle().clone(), state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::complete_onboarding,
            commands::get_settings,
            commands::update_settings,
            commands::default_settings,
            commands::probe_repository,
            commands::add_project,
            commands::list_projects,
            commands::remove_project,
            commands::set_auto_protect,
            commands::set_monitoring,
            commands::refresh_project,
            commands::list_worktrees,
            commands::worktree_guidance,
            commands::create_snapshot,
            commands::cancel_task,
            commands::query_snapshots,
            commands::snapshot_detail,
            commands::verify_snapshot,
            commands::set_snapshot_pinned,
            commands::set_snapshot_note,
            commands::delete_snapshots,
            commands::file_history,
            commands::blob_content,
            commands::git_blob_content,
            commands::current_file_content,
            commands::suggest_recovery_target,
            commands::recover_to_directory,
            commands::preview_restore,
            commands::execute_restore,
            commands::list_recoveries,
            commands::forensic_scan,
            commands::forensic_commit_files,
            commands::recover_git_object,
            commands::suggest_forensic_target,
            commands::list_risks,
            commands::acknowledge_risk,
            commands::query_events,
            commands::storage_stats,
            commands::retention_plan,
            commands::run_retention,
            commands::build_demo,
            commands::export_diagnostics,
            commands::reveal_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Worktree Rescue");
}
