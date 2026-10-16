//! SQLite persistence: schema migrations and typed queries.

use crate::error::{CoreError, Result};
use crate::manifest::Manifest;
use crate::settings::Settings;
use crate::types::*;
use rusqlite::{Connection, OptionalExtension, Row, params};
use std::path::Path;

const MIGRATIONS: &[&str] = &[
    // v1
    r#"
CREATE TABLE projects(
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  root_path TEXT NOT NULL,
  common_dir TEXT NOT NULL,
  common_key TEXT NOT NULL UNIQUE,
  is_demo INTEGER NOT NULL DEFAULT 0,
  added_at INTEGER NOT NULL,
  auto_protect INTEGER NOT NULL DEFAULT 0,
  monitoring TEXT NOT NULL DEFAULT 'active'
);
CREATE TABLE worktrees(
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  path TEXT NOT NULL,
  path_key TEXT NOT NULL,
  is_main INTEGER NOT NULL DEFAULT 0,
  branch TEXT,
  head_oid TEXT,
  detached INTEGER NOT NULL DEFAULT 0,
  locked INTEGER NOT NULL DEFAULT 0,
  lock_reason TEXT,
  git_prunable INTEGER NOT NULL DEFAULT 0,
  prunable_reason TEXT,
  diagnosis TEXT NOT NULL DEFAULT 'unknown',
  evidence TEXT NOT NULL DEFAULT '[]',
  possible_new_path TEXT,
  dirty INTEGER,
  counts TEXT,
  first_seen_at INTEGER NOT NULL,
  last_seen_at INTEGER,
  last_checked_at INTEGER NOT NULL,
  last_dirty_at INTEGER,
  dirty_fingerprint TEXT,
  last_confirmed_at INTEGER,
  gone_from_git INTEGER NOT NULL DEFAULT 0,
  UNIQUE(project_id, path_key)
);
CREATE TABLE snapshots(
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  worktree_path TEXT NOT NULL,
  worktree_key TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  finished_at INTEGER,
  trigger TEXT NOT NULL,
  status TEXT NOT NULL,
  branch TEXT,
  head_oid TEXT,
  detached INTEGER NOT NULL DEFAULT 0,
  entry_count INTEGER NOT NULL DEFAULT 0,
  captured_count INTEGER NOT NULL DEFAULT 0,
  skipped_count INTEGER NOT NULL DEFAULT 0,
  failed_count INTEGER NOT NULL DEFAULT 0,
  staged_count INTEGER NOT NULL DEFAULT 0,
  unstaged_count INTEGER NOT NULL DEFAULT 0,
  untracked_count INTEGER NOT NULL DEFAULT 0,
  deleted_count INTEGER NOT NULL DEFAULT 0,
  protected_bytes INTEGER NOT NULL DEFAULT 0,
  new_bytes INTEGER NOT NULL DEFAULT 0,
  manifest_sha256 TEXT,
  content_key TEXT,
  pinned INTEGER NOT NULL DEFAULT 0,
  note TEXT,
  error_code TEXT,
  error_message TEXT,
  verified_at INTEGER
);
CREATE INDEX idx_snapshots_project ON snapshots(project_id, created_at DESC);
CREATE INDEX idx_snapshots_worktree ON snapshots(project_id, worktree_key, created_at DESC);
CREATE TABLE snapshot_entries(
  snapshot_id TEXT NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
  path TEXT NOT NULL,
  orig_path TEXT,
  category TEXT NOT NULL,
  x TEXT NOT NULL,
  y TEXT NOT NULL,
  kind TEXT NOT NULL,
  wt_state TEXT NOT NULL,
  wt_blob TEXT,
  wt_size INTEGER,
  index_blob TEXT,
  index_oid TEXT,
  head_oid TEXT,
  skip_reason TEXT,
  link_target TEXT,
  lfs_pointer INTEGER NOT NULL DEFAULT 0,
  is_binary INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY(snapshot_id, path)
);
CREATE INDEX idx_entries_path ON snapshot_entries(path);
CREATE INDEX idx_entries_wt_blob ON snapshot_entries(wt_blob);
CREATE INDEX idx_entries_index_blob ON snapshot_entries(index_blob);
CREATE TABLE blobs(
  sha256 TEXT PRIMARY KEY,
  size INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  verified_at INTEGER
);
CREATE TABLE events(
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  worktree_path TEXT,
  at INTEGER NOT NULL,
  kind TEXT NOT NULL,
  snapshot_id TEXT,
  data TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX idx_events_project ON events(project_id, at DESC);
CREATE TABLE risks(
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  worktree_path TEXT,
  rule TEXT NOT NULL,
  dedupe_key TEXT NOT NULL,
  severity TEXT NOT NULL,
  confidence TEXT NOT NULL,
  detected_at INTEGER NOT NULL,
  last_seen_at INTEGER NOT NULL,
  resolved_at INTEGER,
  acknowledged INTEGER NOT NULL DEFAULT 0,
  evidence TEXT NOT NULL DEFAULT '{}'
);
CREATE UNIQUE INDEX idx_risks_open ON risks(project_id, dedupe_key) WHERE resolved_at IS NULL;
CREATE TABLE recoveries(
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  snapshot_id TEXT,
  source TEXT NOT NULL,
  mode TEXT NOT NULL,
  target TEXT NOT NULL,
  started_at INTEGER NOT NULL,
  finished_at INTEGER,
  status TEXT NOT NULL,
  report TEXT
);
CREATE TABLE settings(
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
"#,
];

pub struct Store {
    conn: Connection,
}

/// Normalised key for path identity (case-insensitive on Windows).
pub fn path_key(p: &str) -> String {
    let simplified = crate::paths::display(Path::new(p));
    let s = simplified.trim_end_matches(['\\', '/']).to_string();
    if cfg!(windows) {
        s.replace('/', "\\").to_lowercase()
    } else {
        s
    }
}

fn b(v: bool) -> i64 {
    v as i64
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;",
        )?;
        let mut s = Store { conn };
        s.migrate()?;
        Ok(s)
    }

    fn migrate(&mut self) -> Result<()> {
        let version: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate() {
            let target = i as i64 + 1;
            if version < target {
                let tx = self.conn.transaction()?;
                tx.execute_batch(sql)?;
                tx.execute_batch(&format!("PRAGMA user_version = {target}"))?;
                tx.commit()?;
            }
        }
        Ok(())
    }

    pub fn schema_version(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    // ------------------------------------------------------------------
    // Settings
    // ------------------------------------------------------------------

    pub fn load_settings(&self) -> Result<Settings> {
        let raw: Option<String> = self
            .conn
            .query_row("SELECT value FROM settings WHERE key='app'", [], |r| {
                r.get(0)
            })
            .optional()?;
        match raw {
            Some(s) => Ok(serde_json::from_str(&s).unwrap_or_default()),
            None => Ok(Settings::default()),
        }
    }

    pub fn save_settings(&self, s: &Settings) -> Result<()> {
        s.validate()?;
        self.conn.execute(
            "INSERT INTO settings(key,value) VALUES('app',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![serde_json::to_string(s)?],
        )?;
        Ok(())
    }

    pub fn get_kv(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM settings WHERE key=?1", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn set_kv(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Projects
    // ------------------------------------------------------------------

    pub fn insert_project(
        &self,
        id: &str,
        name: &str,
        root: &str,
        common_dir: &str,
        is_demo: bool,
        auto_protect: bool,
    ) -> Result<()> {
        let key = path_key(common_dir);
        let exists: Option<String> = self
            .conn
            .query_row("SELECT id FROM projects WHERE common_key=?1", [&key], |r| {
                r.get(0)
            })
            .optional()?;
        if exists.is_some() {
            return Err(CoreError::AlreadyRegistered(root.to_string()));
        }
        self.conn.execute(
            "INSERT INTO projects(id,name,root_path,common_dir,common_key,is_demo,added_at,auto_protect,monitoring)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'active')",
            params![id, name, root, common_dir, key, b(is_demo), crate::now_ms(), b(auto_protect)],
        )?;
        Ok(())
    }

    pub fn find_project_by_common_dir(&self, common_dir: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id FROM projects WHERE common_key=?1",
                [path_key(common_dir)],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn list_projects(&self) -> Result<Vec<Project>> {
        let mut stmt = self.conn.prepare(
            "SELECT p.id,p.name,p.root_path,p.common_dir,p.is_demo,p.added_at,p.auto_protect,p.monitoring,
               (SELECT MAX(created_at) FROM snapshots s WHERE s.project_id=p.id AND s.status IN ('verified','partial')),
               (SELECT MAX(created_at) FROM snapshots s WHERE s.project_id=p.id AND s.status='verified'),
               (SELECT COUNT(*) FROM snapshots s WHERE s.project_id=p.id),
               (SELECT COALESCE(SUM(new_bytes),0) FROM snapshots s WHERE s.project_id=p.id),
               (SELECT COUNT(*) FROM risks r WHERE r.project_id=p.id AND r.resolved_at IS NULL AND r.severity='info'),
               (SELECT COUNT(*) FROM risks r WHERE r.project_id=p.id AND r.resolved_at IS NULL AND r.severity='attention'),
               (SELECT COUNT(*) FROM risks r WHERE r.project_id=p.id AND r.resolved_at IS NULL AND r.severity='critical')
             FROM projects p ORDER BY p.is_demo, p.added_at",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Project {
                id: r.get(0)?,
                name: r.get(1)?,
                root_path: r.get(2)?,
                common_dir: r.get(3)?,
                is_demo: r.get::<_, i64>(4)? != 0,
                added_at: r.get(5)?,
                auto_protect: r.get::<_, i64>(6)? != 0,
                monitoring: Monitoring::parse(&r.get::<_, String>(7)?),
                last_snapshot_at: r.get(8)?,
                last_verified_snapshot_at: r.get(9)?,
                snapshot_count: r.get(10)?,
                stored_bytes: r.get(11)?,
                open_risks: RiskCounts {
                    info: r.get(12)?,
                    attention: r.get(13)?,
                    critical: r.get(14)?,
                },
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn get_project(&self, id: &str) -> Result<Project> {
        self.list_projects()?
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| CoreError::ProjectNotFound(id.to_string()))
    }

    pub fn set_project_auto_protect(&self, id: &str, on: bool) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE projects SET auto_protect=?2 WHERE id=?1",
            params![id, b(on)],
        )?;
        if n == 0 {
            return Err(CoreError::ProjectNotFound(id.into()));
        }
        Ok(())
    }

    pub fn set_project_monitoring(&self, id: &str, m: Monitoring) -> Result<()> {
        self.conn.execute(
            "UPDATE projects SET monitoring=?2 WHERE id=?1",
            params![id, m.as_str()],
        )?;
        Ok(())
    }

    pub fn rename_project(&self, id: &str, name: &str) -> Result<()> {
        self.conn
            .execute("UPDATE projects SET name=?2 WHERE id=?1", params![id, name])?;
        Ok(())
    }

    /// Delete a project row and everything cascading from it. Blob files are
    /// garbage-collected separately after reference checks.
    pub fn delete_project(&mut self, id: &str) -> Result<Vec<String>> {
        let tx = self.conn.transaction()?;
        let ids: Vec<String> = {
            let mut stmt = tx.prepare("SELECT id FROM snapshots WHERE project_id=?1")?;
            stmt.query_map([id], |r| r.get(0))?
                .collect::<std::result::Result<_, _>>()?
        };
        tx.execute("DELETE FROM recoveries WHERE project_id=?1", [id])?;
        tx.execute("DELETE FROM projects WHERE id=?1", [id])?;
        tx.commit()?;
        Ok(ids)
    }

    // ------------------------------------------------------------------
    // Worktrees
    // ------------------------------------------------------------------

    pub fn list_worktrees(&self, project_id: &str) -> Result<Vec<Worktree>> {
        let mut stmt = self.conn.prepare(
            "SELECT w.id,w.project_id,w.path,w.is_main,w.branch,w.head_oid,w.detached,w.locked,w.lock_reason,
                    w.git_prunable,w.prunable_reason,w.diagnosis,w.evidence,w.possible_new_path,w.dirty,w.counts,
                    w.first_seen_at,w.last_seen_at,w.last_checked_at,w.last_dirty_at,w.gone_from_git,
                    w.dirty_fingerprint,w.last_confirmed_at,
                    (SELECT id FROM snapshots s WHERE s.project_id=w.project_id AND s.worktree_key=w.path_key AND s.status IN ('verified','partial') ORDER BY created_at DESC LIMIT 1),
                    (SELECT MAX(created_at) FROM snapshots s WHERE s.project_id=w.project_id AND s.worktree_key=w.path_key AND s.status IN ('verified','partial')),
                    (SELECT MAX(created_at) FROM snapshots s WHERE s.project_id=w.project_id AND s.worktree_key=w.path_key AND s.status='verified')
             FROM worktrees w WHERE w.project_id=?1 ORDER BY w.is_main DESC, w.gone_from_git, w.first_seen_at",
        )?;
        let rows = stmt.query_map([project_id], |r| {
            let evidence: String = r.get(12)?;
            let counts: Option<String> = r.get(15)?;
            Ok(Worktree {
                id: r.get(0)?,
                project_id: r.get(1)?,
                path: r.get(2)?,
                is_main: r.get::<_, i64>(3)? != 0,
                branch: r.get(4)?,
                head_oid: r.get(5)?,
                detached: r.get::<_, i64>(6)? != 0,
                locked: r.get::<_, i64>(7)? != 0,
                lock_reason: r.get(8)?,
                git_prunable: r.get::<_, i64>(9)? != 0,
                prunable_reason: r.get(10)?,
                diagnosis: WorktreeDiagnosis::parse(&r.get::<_, String>(11)?),
                evidence: serde_json::from_str(&evidence).unwrap_or_default(),
                possible_new_path: r.get(13)?,
                dirty: r.get::<_, Option<i64>>(14)?.map(|v| v != 0),
                counts: counts.and_then(|c| serde_json::from_str(&c).ok()),
                first_seen_at: r.get(16)?,
                last_seen_at: r.get(17)?,
                last_checked_at: r.get(18)?,
                last_dirty_at: r.get(19)?,
                gone_from_git: r.get::<_, i64>(20)? != 0,
                dirty_fingerprint: r.get(21)?,
                last_confirmed_at: r.get(22)?,
                last_snapshot_id: r.get(23)?,
                last_snapshot_at: r.get(24)?,
                last_verified_snapshot_at: r.get(25)?,
                last_protected_at: None,
            })
        })?;
        let mut list: Vec<Worktree> = rows.collect::<std::result::Result<_, _>>()?;
        for w in &mut list {
            w.last_protected_at = match (w.last_snapshot_at, w.last_confirmed_at) {
                (Some(a), Some(b)) => Some(a.max(b)),
                (a, b) => a.or(b),
            };
        }
        Ok(list)
    }

    /// An automatic capture found nothing new: the latest snapshot still
    /// covers the worktree as of now.
    pub fn confirm_worktree(&self, project_id: &str, path: &str, at: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE worktrees SET last_confirmed_at=?3 WHERE project_id=?1 AND path_key=?2",
            params![project_id, path_key(path), at],
        )?;
        Ok(())
    }

    /// The file watcher observed a change in this worktree at `at`.
    pub fn note_worktree_change(&self, project_id: &str, path: &str, at: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE worktrees SET last_dirty_at=MAX(COALESCE(last_dirty_at,0),?3), dirty=1
             WHERE project_id=?1 AND path_key=?2",
            params![project_id, path_key(path), at],
        )?;
        Ok(())
    }

    pub fn upsert_worktree(&self, w: &Worktree) -> Result<()> {
        self.conn.execute(
            "INSERT INTO worktrees(id,project_id,path,path_key,is_main,branch,head_oid,detached,locked,lock_reason,
                git_prunable,prunable_reason,diagnosis,evidence,possible_new_path,dirty,counts,first_seen_at,
                last_seen_at,last_checked_at,last_dirty_at,gone_from_git,dirty_fingerprint)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23)
             ON CONFLICT(project_id,path_key) DO UPDATE SET
                path=excluded.path,is_main=excluded.is_main,branch=excluded.branch,head_oid=excluded.head_oid,
                detached=excluded.detached,locked=excluded.locked,lock_reason=excluded.lock_reason,
                git_prunable=excluded.git_prunable,prunable_reason=excluded.prunable_reason,diagnosis=excluded.diagnosis,
                evidence=excluded.evidence,possible_new_path=excluded.possible_new_path,dirty=excluded.dirty,
                counts=excluded.counts,last_seen_at=excluded.last_seen_at,last_checked_at=excluded.last_checked_at,
                last_dirty_at=excluded.last_dirty_at,gone_from_git=excluded.gone_from_git,
                dirty_fingerprint=excluded.dirty_fingerprint",
            params![
                w.id,
                w.project_id,
                w.path,
                path_key(&w.path),
                b(w.is_main),
                w.branch,
                w.head_oid,
                b(w.detached),
                b(w.locked),
                w.lock_reason,
                b(w.git_prunable),
                w.prunable_reason,
                w.diagnosis.as_str(),
                serde_json::to_string(&w.evidence)?,
                w.possible_new_path,
                w.dirty.map(b),
                w.counts.as_ref().map(serde_json::to_string).transpose()?,
                w.first_seen_at,
                w.last_seen_at,
                w.last_checked_at,
                w.last_dirty_at,
                b(w.gone_from_git),
                w.dirty_fingerprint,
            ],
        )?;
        Ok(())
    }

    pub fn delete_worktree_record(&self, project_id: &str, path: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM worktrees WHERE project_id=?1 AND path_key=?2",
            params![project_id, path_key(path)],
        )?;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Snapshots
    // ------------------------------------------------------------------

    pub fn begin_snapshot(
        &self,
        id: &str,
        project_id: &str,
        worktree_path: &str,
        trigger: SnapshotTrigger,
        created_at: i64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO snapshots(id,project_id,worktree_path,worktree_key,created_at,trigger,status)
             VALUES(?1,?2,?3,?4,?5,?6,'inProgress')",
            params![id, project_id, worktree_path, path_key(worktree_path), created_at, trigger.as_str()],
        )?;
        Ok(())
    }

    /// Commit a finished snapshot: entries, blob rows and summary in one
    /// transaction. Called only after the manifest is durably on disk.
    pub fn commit_snapshot(
        &mut self,
        m: &Manifest,
        manifest_sha: &str,
        status: SnapshotStatus,
        content_key: &str,
        new_blobs: &[(String, u64)],
    ) -> Result<()> {
        let now = crate::now_ms();
        let tx = self.conn.transaction()?;
        for (sha, size) in new_blobs {
            tx.execute(
                "INSERT INTO blobs(sha256,size,created_at,verified_at) VALUES(?1,?2,?3,?3)
                 ON CONFLICT(sha256) DO UPDATE SET verified_at=excluded.verified_at",
                params![sha, *size as i64, now],
            )?;
        }
        {
            let mut ins = tx.prepare(
                "INSERT INTO snapshot_entries(snapshot_id,path,orig_path,category,x,y,kind,wt_state,wt_blob,wt_size,
                    index_blob,index_oid,head_oid,skip_reason,link_target,lfs_pointer,is_binary)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
            )?;
            for e in &m.entries {
                ins.execute(params![
                    m.snapshot_id,
                    e.path,
                    e.orig_path,
                    e.category,
                    e.x,
                    e.y,
                    e.worktree.kind.as_str(),
                    e.worktree.state.as_str(),
                    e.worktree.blob,
                    e.worktree.size.map(|s| s as i64),
                    e.index.as_ref().and_then(|i| i.blob.clone()),
                    e.index.as_ref().map(|i| i.git_oid.clone()),
                    e.head_oid,
                    e.worktree.skip_reason,
                    e.worktree.link_target,
                    b(e.worktree.lfs_pointer),
                    b(e.worktree.binary),
                ])?;
            }
        }
        let cnt = |pred: &dyn Fn(&crate::manifest::ManifestEntry) -> bool| {
            m.entries.iter().filter(|e| pred(e)).count() as i64
        };
        let staged = cnt(&|e| e.x != "." && e.x != "?" && e.category != "conflicted");
        let unstaged = cnt(&|e| e.y != "." && e.y != "?" && e.category != "conflicted");
        let untracked = cnt(&|e| e.category == "untracked" || e.category == "ignoredIncluded");
        let deleted = cnt(&|e| e.worktree.state == crate::manifest::WtState::Absent);
        tx.execute(
            "UPDATE snapshots SET finished_at=?2,status=?3,branch=?4,head_oid=?5,detached=?6,entry_count=?7,
                captured_count=?8,skipped_count=?9,failed_count=?10,staged_count=?11,unstaged_count=?12,
                untracked_count=?13,deleted_count=?14,protected_bytes=?15,new_bytes=?16,manifest_sha256=?17,
                content_key=?18,verified_at=?19
             WHERE id=?1",
            params![
                m.snapshot_id,
                m.finished_at,
                status.as_str(),
                m.git.branch,
                m.git.head_oid,
                b(m.git.detached),
                m.stats.entries as i64,
                m.stats.captured as i64,
                m.stats.skipped as i64,
                m.stats.failed as i64,
                staged,
                unstaged,
                untracked,
                deleted,
                m.stats.protected_bytes as i64,
                m.stats.new_bytes as i64,
                manifest_sha,
                content_key,
                now,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn fail_snapshot(&self, id: &str, code: &str, message: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE snapshots SET status='failed',finished_at=?2,error_code=?3,error_message=?4 WHERE id=?1",
            params![id, crate::now_ms(), code, message],
        )?;
        Ok(())
    }

    /// Mark snapshots left `inProgress` by a crash as interrupted.
    pub fn mark_interrupted(&self) -> Result<usize> {
        Ok(self.conn.execute(
            "UPDATE snapshots SET status='interrupted',finished_at=COALESCE(finished_at,created_at)
             WHERE status='inProgress'",
            [],
        )?)
    }

    pub fn discard_snapshot_row(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM snapshots WHERE id=?1", [id])?;
        Ok(())
    }

    fn map_snapshot(r: &Row) -> rusqlite::Result<SnapshotSummary> {
        Ok(SnapshotSummary {
            id: r.get(0)?,
            project_id: r.get(1)?,
            worktree_path: r.get(2)?,
            created_at: r.get(3)?,
            finished_at: r.get(4)?,
            trigger: SnapshotTrigger::parse(&r.get::<_, String>(5)?),
            status: SnapshotStatus::parse(&r.get::<_, String>(6)?),
            branch: r.get(7)?,
            head_oid: r.get(8)?,
            detached: r.get::<_, i64>(9)? != 0,
            entry_count: r.get(10)?,
            captured_count: r.get(11)?,
            skipped_count: r.get(12)?,
            failed_count: r.get(13)?,
            staged_count: r.get(14)?,
            unstaged_count: r.get(15)?,
            untracked_count: r.get(16)?,
            deleted_count: r.get(17)?,
            protected_bytes: r.get(18)?,
            new_bytes: r.get(19)?,
            manifest_sha256: r.get(20)?,
            pinned: r.get::<_, i64>(21)? != 0,
            note: r.get(22)?,
            error_code: r.get(23)?,
            error_message: r.get(24)?,
            verified_at: r.get(25)?,
        })
    }

    const SNAP_COLS: &'static str = "s.id,s.project_id,s.worktree_path,s.created_at,s.finished_at,s.trigger,s.status,s.branch,s.head_oid,
        s.detached,s.entry_count,s.captured_count,s.skipped_count,s.failed_count,s.staged_count,s.unstaged_count,
        s.untracked_count,s.deleted_count,s.protected_bytes,s.new_bytes,s.manifest_sha256,s.pinned,s.note,
        s.error_code,s.error_message,s.verified_at";

    pub fn get_snapshot(&self, id: &str) -> Result<SnapshotSummary> {
        let sql = format!("SELECT {} FROM snapshots s WHERE s.id=?1", Self::SNAP_COLS);
        self.conn
            .query_row(&sql, [id], Self::map_snapshot)
            .optional()?
            .ok_or_else(|| CoreError::SnapshotNotFound(id.into()))
    }

    pub fn query_snapshots(&self, q: &SnapshotQuery) -> Result<Vec<SnapshotSummary>> {
        let mut sql = format!(
            "SELECT {} FROM snapshots s WHERE s.project_id=?1",
            Self::SNAP_COLS
        );
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(q.project_id.clone())];
        if let Some(w) = &q.worktree_path {
            args.push(Box::new(path_key(w)));
            sql += &format!(" AND s.worktree_key=?{}", args.len());
        }
        if let Some(since) = q.since {
            args.push(Box::new(since));
            sql += &format!(" AND s.created_at>=?{}", args.len());
        }
        if let Some(until) = q.until {
            args.push(Box::new(until));
            sql += &format!(" AND s.created_at<=?{}", args.len());
        }
        if let Some(p) = q.path_contains.as_ref().filter(|p| !p.trim().is_empty()) {
            args.push(Box::new(format!("%{}%", p.trim().replace('\\', "/"))));
            sql += &format!(
                " AND EXISTS(SELECT 1 FROM snapshot_entries e WHERE e.snapshot_id=s.id AND e.path LIKE ?{})",
                args.len()
            );
        }
        sql += " ORDER BY s.created_at DESC";
        args.push(Box::new(q.limit.unwrap_or(500).clamp(1, 5000)));
        sql += &format!(" LIMIT ?{}", args.len());
        let mut stmt = self.conn.prepare(&sql)?;
        let refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|a| a.as_ref()).collect();
        let rows = stmt.query_map(refs.as_slice(), Self::map_snapshot)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn last_content_key(
        &self,
        project_id: &str,
        worktree_path: &str,
    ) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT content_key FROM snapshots WHERE project_id=?1 AND worktree_key=?2
                   AND status IN ('verified','partial') ORDER BY created_at DESC LIMIT 1",
                params![project_id, path_key(worktree_path)],
                |r| r.get(0),
            )
            .optional()?
            .flatten())
    }

    pub fn snapshot_files(&self, snapshot_id: &str) -> Result<Vec<SnapshotFile>> {
        let mut stmt = self.conn.prepare(
            "SELECT path,orig_path,category,x,y,kind,wt_state,wt_blob,wt_size,index_blob,index_oid,head_oid,
                    skip_reason,link_target,lfs_pointer,is_binary
             FROM snapshot_entries WHERE snapshot_id=?1 ORDER BY path",
        )?;
        let rows = stmt.query_map([snapshot_id], Self::map_file)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    fn map_file(r: &Row) -> rusqlite::Result<SnapshotFile> {
        Ok(SnapshotFile {
            path: r.get(0)?,
            orig_path: r.get(1)?,
            category: r.get(2)?,
            x: r.get(3)?,
            y: r.get(4)?,
            kind: r.get(5)?,
            wt_state: r.get(6)?,
            wt_blob: r.get(7)?,
            wt_size: r.get(8)?,
            index_blob: r.get(9)?,
            index_oid: r.get(10)?,
            head_oid: r.get(11)?,
            skip_reason: r.get(12)?,
            link_target: r.get(13)?,
            lfs_pointer: r.get::<_, i64>(14)? != 0,
            binary: r.get::<_, i64>(15)? != 0,
        })
    }

    pub fn file_history(
        &self,
        project_id: &str,
        path: &str,
        limit: i64,
    ) -> Result<Vec<FileVersion>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id,s.created_at,s.worktree_path,s.branch,s.status,
                    e.path,e.orig_path,e.category,e.x,e.y,e.kind,e.wt_state,e.wt_blob,e.wt_size,e.index_blob,
                    e.index_oid,e.head_oid,e.skip_reason,e.link_target,e.lfs_pointer,e.is_binary
             FROM snapshot_entries e JOIN snapshots s ON s.id=e.snapshot_id
             WHERE s.project_id=?1 AND (e.path=?2 OR e.orig_path=?2) AND s.status IN ('verified','partial')
             ORDER BY s.created_at DESC LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![project_id, path, limit], |r| {
            Ok(FileVersion {
                snapshot_id: r.get(0)?,
                created_at: r.get(1)?,
                worktree_path: r.get(2)?,
                branch: r.get(3)?,
                status: SnapshotStatus::parse(&r.get::<_, String>(4)?),
                file: SnapshotFile {
                    path: r.get(5)?,
                    orig_path: r.get(6)?,
                    category: r.get(7)?,
                    x: r.get(8)?,
                    y: r.get(9)?,
                    kind: r.get(10)?,
                    wt_state: r.get(11)?,
                    wt_blob: r.get(12)?,
                    wt_size: r.get(13)?,
                    index_blob: r.get(14)?,
                    index_oid: r.get(15)?,
                    head_oid: r.get(16)?,
                    skip_reason: r.get(17)?,
                    link_target: r.get(18)?,
                    lfs_pointer: r.get::<_, i64>(19)? != 0,
                    binary: r.get::<_, i64>(20)? != 0,
                },
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn skip_counts(&self, snapshot_id: &str) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT skip_reason, COUNT(*) FROM snapshot_entries WHERE snapshot_id=?1 AND skip_reason IS NOT NULL
             GROUP BY skip_reason",
        )?;
        let rows = stmt.query_map([snapshot_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn set_pinned(&self, id: &str, pinned: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE snapshots SET pinned=?2 WHERE id=?1",
            params![id, b(pinned)],
        )?;
        Ok(())
    }

    pub fn set_note(&self, id: &str, note: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE snapshots SET note=?2 WHERE id=?1",
            params![id, note],
        )?;
        Ok(())
    }

    pub fn mark_verified(&self, id: &str, at: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE snapshots SET verified_at=?2 WHERE id=?1",
            params![id, at],
        )?;
        Ok(())
    }

    pub fn delete_snapshots(&mut self, ids: &[String]) -> Result<()> {
        let tx = self.conn.transaction()?;
        for id in ids {
            tx.execute("DELETE FROM snapshots WHERE id=?1", [id])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Blob ids that are no longer referenced by any snapshot entry.
    pub fn unreferenced_blobs(&self) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT b.sha256,b.size FROM blobs b
             WHERE NOT EXISTS(SELECT 1 FROM snapshot_entries e WHERE e.wt_blob=b.sha256)
               AND NOT EXISTS(SELECT 1 FROM snapshot_entries e WHERE e.index_blob=b.sha256)",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn is_blob_referenced(&self, sha: &str) -> Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM snapshot_entries WHERE wt_blob=?1 OR index_blob=?1",
            [sha],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    pub fn delete_blob_row(&self, sha: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM blobs WHERE sha256=?1", [sha])?;
        Ok(())
    }

    pub fn known_blob(&self, sha: &str) -> Result<bool> {
        let n: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM blobs WHERE sha256=?1", [sha], |r| {
                    r.get(0)
                })?;
        Ok(n > 0)
    }

    pub fn blob_totals(&self) -> Result<(i64, i64)> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(size),0) FROM blobs",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    }

    /// Bytes of distinct blobs referenced by a project's snapshots.
    pub fn project_referenced_bytes(&self, project_id: &str) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(SUM(size),0) FROM blobs WHERE sha256 IN (
               SELECT e.wt_blob FROM snapshot_entries e JOIN snapshots s ON s.id=e.snapshot_id
                 WHERE s.project_id=?1 AND e.wt_blob IS NOT NULL
               UNION
               SELECT e.index_blob FROM snapshot_entries e JOIN snapshots s ON s.id=e.snapshot_id
                 WHERE s.project_id=?1 AND e.index_blob IS NOT NULL)",
            [project_id],
            |r| r.get(0),
        )?)
    }

    pub fn snapshot_count(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM snapshots", [], |r| r.get(0))?)
    }

    pub fn consecutive_auto_failures(&self, project_id: &str, worktree_path: &str) -> Result<i64> {
        let mut stmt = self.conn.prepare(
            "SELECT status FROM snapshots WHERE project_id=?1 AND worktree_key=?2 AND trigger IN ('auto','checkpoint')
             ORDER BY created_at DESC LIMIT 20",
        )?;
        let statuses: Vec<String> = stmt
            .query_map(params![project_id, path_key(worktree_path)], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(statuses
            .iter()
            .take_while(|s| s.as_str() == "failed" || s.as_str() == "interrupted")
            .count() as i64)
    }

    /// Snapshots eligible for retention evaluation (oldest first).
    pub fn snapshots_for_retention(&self, project_id: &str) -> Result<Vec<SnapshotSummary>> {
        let sql = format!(
            "SELECT {} FROM snapshots s WHERE s.project_id=?1 ORDER BY s.created_at ASC",
            Self::SNAP_COLS
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([project_id], Self::map_snapshot)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    // ------------------------------------------------------------------
    // Events
    // ------------------------------------------------------------------

    pub fn add_event(
        &self,
        project_id: &str,
        worktree_path: Option<&str>,
        kind: &str,
        snapshot_id: Option<&str>,
        data: serde_json::Value,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO events(project_id,worktree_path,at,kind,snapshot_id,data) VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                project_id,
                worktree_path,
                crate::now_ms(),
                kind,
                snapshot_id,
                data.to_string()
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn query_events(&self, q: &EventQuery) -> Result<Vec<TimelineEvent>> {
        let mut sql = String::from(
            "SELECT id,project_id,worktree_path,at,kind,snapshot_id,data FROM events WHERE project_id=?1",
        );
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(q.project_id.clone())];
        if let Some(w) = &q.worktree_path {
            args.push(Box::new(w.clone()));
            sql += &format!(" AND worktree_path=?{}", args.len());
        }
        if let Some(since) = q.since {
            args.push(Box::new(since));
            sql += &format!(" AND at>=?{}", args.len());
        }
        if let Some(until) = q.until {
            args.push(Box::new(until));
            sql += &format!(" AND at<=?{}", args.len());
        }
        if let Some(kinds) = q.kinds.as_ref().filter(|k| !k.is_empty()) {
            let mut ph = Vec::new();
            for k in kinds {
                args.push(Box::new(k.clone()));
                ph.push(format!("?{}", args.len()));
            }
            sql += &format!(" AND kind IN ({})", ph.join(","));
        }
        sql += " ORDER BY at DESC, id DESC";
        args.push(Box::new(q.limit.unwrap_or(500).clamp(1, 5000)));
        sql += &format!(" LIMIT ?{}", args.len());
        let mut stmt = self.conn.prepare(&sql)?;
        let refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|a| a.as_ref()).collect();
        let rows = stmt.query_map(refs.as_slice(), |r| {
            let data: String = r.get(6)?;
            Ok(TimelineEvent {
                id: r.get(0)?,
                project_id: r.get(1)?,
                worktree_path: r.get(2)?,
                at: r.get(3)?,
                kind: r.get(4)?,
                snapshot_id: r.get(5)?,
                data: serde_json::from_str(&data).unwrap_or(serde_json::Value::Null),
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    // ------------------------------------------------------------------
    // Risks
    // ------------------------------------------------------------------

    /// Insert or refresh an open risk identified by `dedupe_key`.
    /// Returns true if it is newly raised.
    pub fn raise_risk(&self, r: &Risk, dedupe_key: &str) -> Result<bool> {
        let existing: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM risks WHERE project_id=?1 AND dedupe_key=?2 AND resolved_at IS NULL",
                params![r.project_id, dedupe_key],
                |row| row.get(0),
            )
            .optional()?;
        match existing {
            Some(id) => {
                self.conn.execute(
                    "UPDATE risks SET last_seen_at=?2,severity=?3,confidence=?4,evidence=?5 WHERE id=?1",
                    params![
                        id,
                        r.last_seen_at,
                        r.severity.as_str(),
                        r.confidence.as_str(),
                        r.evidence.to_string()
                    ],
                )?;
                Ok(false)
            }
            None => {
                self.conn.execute(
                    "INSERT INTO risks(id,project_id,worktree_path,rule,dedupe_key,severity,confidence,detected_at,
                        last_seen_at,acknowledged,evidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,0,?10)",
                    params![
                        r.id,
                        r.project_id,
                        r.worktree_path,
                        r.rule,
                        dedupe_key,
                        r.severity.as_str(),
                        r.confidence.as_str(),
                        r.detected_at,
                        r.last_seen_at,
                        r.evidence.to_string()
                    ],
                )?;
                Ok(true)
            }
        }
    }

    /// Resolve open risks of `rules` for a project whose dedupe key is not in
    /// `still_active`. Returns resolved risks.
    pub fn resolve_missing_risks(
        &self,
        project_id: &str,
        rules: &[&str],
        still_active: &[String],
    ) -> Result<Vec<Risk>> {
        let open = self.list_risks(project_id, false)?;
        let mut resolved = Vec::new();
        let now = crate::now_ms();
        for r in open {
            if !rules.contains(&r.rule.as_str()) {
                continue;
            }
            let key: String = self.conn.query_row(
                "SELECT dedupe_key FROM risks WHERE id=?1",
                [&r.id],
                |row| row.get(0),
            )?;
            if !still_active.contains(&key) {
                self.conn.execute(
                    "UPDATE risks SET resolved_at=?2 WHERE id=?1",
                    params![r.id, now],
                )?;
                resolved.push(r);
            }
        }
        Ok(resolved)
    }

    pub fn list_risks(&self, project_id: &str, include_resolved: bool) -> Result<Vec<Risk>> {
        let sql = if include_resolved {
            "SELECT id,project_id,worktree_path,rule,severity,confidence,detected_at,last_seen_at,resolved_at,acknowledged,evidence
             FROM risks WHERE project_id=?1 ORDER BY resolved_at IS NOT NULL, detected_at DESC LIMIT 500"
        } else {
            "SELECT id,project_id,worktree_path,rule,severity,confidence,detected_at,last_seen_at,resolved_at,acknowledged,evidence
             FROM risks WHERE project_id=?1 AND resolved_at IS NULL ORDER BY detected_at DESC"
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map([project_id], |r| {
            let ev: String = r.get(10)?;
            Ok(Risk {
                id: r.get(0)?,
                project_id: r.get(1)?,
                worktree_path: r.get(2)?,
                rule: r.get(3)?,
                severity: Severity::parse(&r.get::<_, String>(4)?),
                confidence: Confidence::parse(&r.get::<_, String>(5)?),
                detected_at: r.get(6)?,
                last_seen_at: r.get(7)?,
                resolved_at: r.get(8)?,
                acknowledged: r.get::<_, i64>(9)? != 0,
                evidence: serde_json::from_str(&ev).unwrap_or(serde_json::Value::Null),
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn acknowledge_risk(&self, id: &str) -> Result<()> {
        self.conn
            .execute("UPDATE risks SET acknowledged=1 WHERE id=?1", [id])?;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Recoveries
    // ------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub fn record_recovery(
        &self,
        id: &str,
        project_id: &str,
        snapshot_id: Option<&str>,
        source: &str,
        mode: &str,
        target: &str,
        started_at: i64,
        status: &str,
        report: &serde_json::Value,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO recoveries(id,project_id,snapshot_id,source,mode,target,started_at,finished_at,status,report)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                id,
                project_id,
                snapshot_id,
                source,
                mode,
                target,
                started_at,
                crate::now_ms(),
                status,
                report.to_string()
            ],
        )?;
        Ok(())
    }

    pub fn list_recoveries(&self, project_id: &str) -> Result<Vec<serde_json::Value>> {
        let mut stmt = self.conn.prepare(
            "SELECT id,snapshot_id,source,mode,target,started_at,finished_at,status,report FROM recoveries
             WHERE project_id=?1 ORDER BY started_at DESC LIMIT 200",
        )?;
        let rows = stmt.query_map([project_id], |r| {
            let report: Option<String> = r.get(8)?;
            Ok(serde_json::json!({
                "id": r.get::<_, String>(0)?,
                "snapshotId": r.get::<_, Option<String>>(1)?,
                "source": r.get::<_, String>(2)?,
                "mode": r.get::<_, String>(3)?,
                "target": r.get::<_, String>(4)?,
                "startedAt": r.get::<_, i64>(5)?,
                "finishedAt": r.get::<_, Option<i64>>(6)?,
                "status": r.get::<_, String>(7)?,
                "report": report.and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()),
            }))
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
}
