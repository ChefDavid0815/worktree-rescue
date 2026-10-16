//! Application state shared by commands and background services.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use wr_core::error::{CoreError, ErrorPayload};
use wr_core::vault::Vault;

pub struct AppState {
    /// Real projects.
    pub main: Arc<Vault>,
    /// Sandbox demo, physically separate from `main`.
    pub demo: Arc<Vault>,
    pub data_dir: PathBuf,
    pub log_path: PathBuf,
    /// Cancellation flags of running tasks, keyed by task id.
    pub tasks: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl AppState {
    pub fn vault_for(&self, project_id: &str) -> Result<Arc<Vault>, ErrorPayload> {
        if self.main.has_project(project_id) {
            Ok(self.main.clone())
        } else if self.demo.has_project(project_id) {
            Ok(self.demo.clone())
        } else {
            Err(err(CoreError::ProjectNotFound(project_id.to_string())))
        }
    }

    /// Find the vault holding a snapshot id.
    pub fn vault_for_snapshot(&self, snapshot_id: &str) -> Result<Arc<Vault>, ErrorPayload> {
        if self.main.store().get_snapshot(snapshot_id).is_ok() {
            Ok(self.main.clone())
        } else if self.demo.store().get_snapshot(snapshot_id).is_ok() {
            Ok(self.demo.clone())
        } else {
            Err(err(CoreError::SnapshotNotFound(snapshot_id.to_string())))
        }
    }

    pub fn vaults(&self) -> [Arc<Vault>; 2] {
        [self.main.clone(), self.demo.clone()]
    }

    pub fn register_task(&self, id: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.tasks
            .lock()
            .unwrap()
            .insert(id.to_string(), flag.clone());
        flag
    }

    pub fn finish_task(&self, id: &str) {
        self.tasks.lock().unwrap().remove(id);
    }
}

pub fn err(e: CoreError) -> ErrorPayload {
    if !matches!(e, CoreError::Busy | CoreError::Cancelled) {
        log::warn!("{}: {}", e.code(), e);
    }
    ErrorPayload::from(&e)
}

pub type CmdResult<T> = Result<T, ErrorPayload>;
