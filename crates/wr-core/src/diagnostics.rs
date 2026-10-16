//! Local environment inspection and diagnostic export. Nothing here leaves
//! the machine; the export is a file the user saves wherever they choose.

use crate::git::Git;
use crate::paths;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentReport {
    pub os: String,
    pub app_version: String,
    pub git_found: bool,
    pub git_path: Option<String>,
    pub git_version: Option<String>,
    /// Worktree Rescue relies on `rev-parse --path-format` (Git >= 2.31).
    pub git_supported: bool,
    pub data_dir: String,
    pub data_dir_writable: bool,
    pub available_bytes: Option<u64>,
    pub recovery_dir: String,
    pub checked_at: i64,
}

/// Parse "git version 2.45.1.windows.1" into (2, 45).
pub fn parse_git_version(s: &str) -> Option<(u32, u32)> {
    let v = s.trim().strip_prefix("git version ")?;
    let mut it = v.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it
        .next()?
        .trim_end_matches(|c: char| !c.is_ascii_digit())
        .parse()
        .ok()?;
    Some((major, minor))
}

pub fn inspect(git: Option<&Git>, data_dir: &Path, recovery_dir: &str) -> EnvironmentReport {
    let version = git.and_then(|g| g.version().ok());
    let supported = version
        .as_deref()
        .and_then(parse_git_version)
        .is_some_and(|(maj, min)| maj > 2 || (maj == 2 && min >= 31));
    let writable = {
        let probe = data_dir.join(format!(".probe-{}", uuid::Uuid::new_v4().simple()));
        let ok = std::fs::create_dir_all(data_dir).is_ok() && std::fs::write(&probe, b"ok").is_ok();
        let _ = std::fs::remove_file(&probe);
        ok
    };
    EnvironmentReport {
        os: crate::platform::os_label(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        git_found: git.is_some(),
        git_path: git.map(|g| paths::display(g.exe())),
        git_version: version,
        git_supported: supported,
        data_dir: paths::display(data_dir),
        data_dir_writable: writable,
        available_bytes: crate::platform::available_space(data_dir),
        recovery_dir: recovery_dir.to_string(),
        checked_at: crate::now_ms(),
    }
}

/// Last `max_lines` lines of a log file.
pub fn tail(path: &Path, max_lines: usize) -> Vec<String> {
    let Ok(s) = std::fs::read_to_string(path) else {
        return vec![];
    };
    let lines: Vec<&str> = s.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].iter().map(|l| l.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_versions() {
        assert_eq!(
            parse_git_version("git version 2.54.0.windows.1"),
            Some((2, 54))
        );
        assert_eq!(parse_git_version("git version 2.30.1"), Some((2, 30)));
        assert_eq!(parse_git_version("nope"), None);
    }
}
