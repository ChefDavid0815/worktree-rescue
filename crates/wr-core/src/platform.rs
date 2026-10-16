//! Platform abstraction. Everything OS-specific lives here so the rest of the
//! crate stays portable (Windows first, macOS/Linux later).

use std::path::{Path, PathBuf};
use std::process::Command;

/// Application identifier used for the data directory.
pub const APP_DIR_NAME: &str = "WorktreeRescue";

/// Default vault root: `%LOCALAPPDATA%\WorktreeRescue` on Windows,
/// `~/Library/Application Support/WorktreeRescue` on macOS and
/// `$XDG_DATA_HOME/WorktreeRescue` on Linux.
pub fn default_data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(APP_DIR_NAME)
}

/// Default location for recovered files (outside every repository).
pub fn default_recovery_dir() -> PathBuf {
    dirs::document_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("Worktree Rescue")
        .join("Recovered")
}

/// Configure a child process so it never flashes a console window.
pub fn hide_console(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = cmd;
    }
}

/// Free bytes on the volume holding `path` (walks up to an existing ancestor).
pub fn available_space(path: &Path) -> Option<u64> {
    let mut p = Some(path);
    while let Some(cur) = p {
        if cur.exists() {
            return fs4::available_space(cur).ok();
        }
        p = cur.parent();
    }
    None
}

/// The root of the volume a path lives on (`C:\`, `\\server\share\`, `/`).
pub fn volume_root(path: &Path) -> Option<PathBuf> {
    use std::path::Component;
    let simplified = dunce::simplified(path);
    let mut comps = simplified.components();
    match comps.next()? {
        Component::Prefix(prefix) => {
            let mut root = PathBuf::from(prefix.as_os_str());
            root.push(std::path::MAIN_SEPARATOR_STR);
            Some(root)
        }
        Component::RootDir => Some(PathBuf::from("/")),
        _ => None,
    }
}

/// Classification of why a path could not be reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reachability {
    /// The path exists.
    Present,
    /// The volume is mounted but the path is gone (or was moved).
    MissingOnVolume,
    /// The volume or network share itself is unavailable; the data may be fine.
    VolumeUnavailable,
    /// Access was denied or another I/O error occurred.
    Inaccessible,
}

pub fn reachability(path: &Path) -> Reachability {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Reachability::Present,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => match volume_root(path) {
            Some(root) if root.exists() => {
                // On Windows an unmounted removable drive often still
                // "exists" as a letter but cannot be listed.
                match std::fs::read_dir(&root) {
                    Ok(_) => Reachability::MissingOnVolume,
                    Err(_) => Reachability::VolumeUnavailable,
                }
            }
            Some(_) => Reachability::VolumeUnavailable,
            None => Reachability::MissingOnVolume,
        },
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Reachability::Inaccessible,
        Err(_) => Reachability::Inaccessible,
    }
}

/// Short platform label for diagnostics.
pub fn os_label() -> String {
    format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)
}

/// Locate a Git executable: explicit override, then PATH, then the standard
/// Git for Windows install locations.
pub fn find_git(override_path: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = override_path
        && p.is_file()
    {
        return Some(p.to_path_buf());
    }
    let exe = if cfg!(windows) { "git.exe" } else { "git" };
    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let cand = dir.join(exe);
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    #[cfg(windows)]
    {
        let mut candidates = vec![
            PathBuf::from(r"C:\Program Files\Git\cmd\git.exe"),
            PathBuf::from(r"C:\Program Files (x86)\Git\cmd\git.exe"),
        ];
        if let Some(local) = dirs::data_local_dir() {
            candidates.push(local.join(r"Programs\Git\cmd\git.exe"));
        }
        for c in candidates {
            if c.is_file() {
                return Some(c);
            }
        }
    }
    None
}
