//! Application settings with validated defaults.

use crate::error::{CoreError, Result};
use serde::{Deserialize, Serialize};

pub const MB: i64 = 1024 * 1024;
pub const GB: i64 = 1024 * MB;

/// Directories and files that are excluded from untracked capture by
/// default (dependency and build caches). Git-ignored files are excluded
/// regardless unless the user adds an explicit include rule.
pub fn default_exclude_globs() -> Vec<String> {
    [
        "**/node_modules/**",
        "**/target/**",
        "**/dist/**",
        "**/build/**",
        "**/.next/**",
        "**/.nuxt/**",
        "**/.turbo/**",
        "**/.cache/**",
        "**/__pycache__/**",
        "**/.venv/**",
        "**/venv/**",
        "**/.gradle/**",
        "**/bin/Debug/**",
        "**/obj/**",
        "**/*.pyc",
        "**/.DS_Store",
        "**/Thumbs.db",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Files that commonly hold secrets. Excluded unless the user opts in;
/// even when included they never leave this machine.
pub fn default_sensitive_globs() -> Vec<String> {
    [
        "**/.env",
        "**/.env.*",
        "**/*.pem",
        "**/*.key",
        "**/*.p12",
        "**/*.pfx",
        "**/id_rsa*",
        "**/id_ed25519*",
        "**/.npmrc",
        "**/.pypirc",
        "**/credentials.json",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct RetentionPolicy {
    /// Keep every snapshot younger than this many hours.
    pub keep_all_hours: i64,
    /// Then keep one snapshot per hour for this many days.
    pub keep_hourly_days: i64,
    /// Then keep one snapshot per day for this many days.
    pub keep_daily_days: i64,
    /// Hard cap per worktree (oldest unpinned pruned first).
    pub max_snapshots_per_worktree: i64,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        RetentionPolicy {
            keep_all_hours: 24,
            keep_hourly_days: 7,
            keep_daily_days: 30,
            max_snapshots_per_worktree: 400,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Whether newly added repositories start with automatic protection on.
    pub auto_protect_new_projects: bool,
    pub quiet_period_secs: i64,
    pub min_interval_secs: i64,
    pub checkpoint_interval_secs: i64,
    pub max_file_bytes: i64,
    pub repo_budget_bytes: i64,
    pub include_untracked: bool,
    pub exclude_globs: Vec<String>,
    pub sensitive_globs: Vec<String>,
    pub include_sensitive: bool,
    /// Git-ignored paths the user explicitly wants protected.
    pub include_ignored_globs: Vec<String>,
    pub recovery_dir: String,
    pub retention: RetentionPolicy,
    pub notify_on_risk: bool,
    pub notify_on_failure: bool,
    pub theme: String,
    pub language: String,
    pub git_path: Option<String>,
    pub stale_protection_minutes: i64,
    pub mass_deletion_threshold: i64,
    pub min_free_space_bytes: i64,
    pub inventory_poll_secs: i64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            auto_protect_new_projects: false,
            quiet_period_secs: 20,
            min_interval_secs: 60,
            checkpoint_interval_secs: 600,
            max_file_bytes: 25 * MB,
            repo_budget_bytes: 2 * GB,
            include_untracked: true,
            exclude_globs: default_exclude_globs(),
            sensitive_globs: default_sensitive_globs(),
            include_sensitive: false,
            include_ignored_globs: vec![],
            recovery_dir: crate::paths::display(&crate::platform::default_recovery_dir()),
            retention: RetentionPolicy::default(),
            notify_on_risk: true,
            notify_on_failure: true,
            theme: "system".into(),
            language: "auto".into(),
            git_path: None,
            stale_protection_minutes: 120,
            mass_deletion_threshold: 25,
            min_free_space_bytes: 2 * GB,
            inventory_poll_secs: 60,
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<()> {
        let bad = |m: &str| Err(CoreError::InvalidSetting(m.to_string()));
        if !(5..=3600).contains(&self.quiet_period_secs) {
            return bad("quietPeriodSecs must be between 5 and 3600");
        }
        if !(30..=86_400).contains(&self.min_interval_secs) {
            return bad("minIntervalSecs must be between 30 and 86400");
        }
        if !(60..=86_400).contains(&self.checkpoint_interval_secs) {
            return bad("checkpointIntervalSecs must be between 60 and 86400");
        }
        if !(1024..=4 * GB).contains(&self.max_file_bytes) {
            return bad("maxFileBytes must be between 1 KB and 4 GB");
        }
        if self.repo_budget_bytes < 64 * MB {
            return bad("repoBudgetBytes must be at least 64 MB");
        }
        if self.retention.keep_all_hours < 1
            || self.retention.keep_hourly_days < 0
            || self.retention.keep_daily_days < 0
            || self.retention.max_snapshots_per_worktree < 10
        {
            return bad("retention values out of range");
        }
        if !matches!(self.theme.as_str(), "system" | "dark" | "light") {
            return bad("theme must be system, dark or light");
        }
        if !matches!(self.language.as_str(), "auto" | "en" | "zh-CN") {
            return bad("language must be auto, en or zh-CN");
        }
        if !(15..=3600).contains(&self.inventory_poll_secs) {
            return bad("inventoryPollSecs must be between 15 and 3600");
        }
        for g in self
            .exclude_globs
            .iter()
            .chain(&self.sensitive_globs)
            .chain(&self.include_ignored_globs)
        {
            globset::Glob::new(g).map_err(|e| CoreError::InvalidSetting(format!("{g}: {e}")))?;
        }
        Ok(())
    }
}
