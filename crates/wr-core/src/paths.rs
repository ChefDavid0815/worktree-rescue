//! Path validation and boundary enforcement.
//!
//! Every path that comes out of Git output or a manifest is treated as
//! untrusted. It must be a *relative*, forward-slash separated path without
//! `.`/`..` components before it can be joined onto a base directory, and the
//! join helpers refuse to traverse symbolic links or Windows reparse points.

use crate::error::{CoreError, Result};
use std::path::{Component, Path, PathBuf};

const WINDOWS_RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Validate a repository-relative path as produced by Git (`a/b/c.txt`).
///
/// Rejects absolute paths, drive prefixes, backslashes, empty components,
/// `.`/`..`, NUL bytes and – because recovery targets may be on Windows –
/// names that Windows would silently reinterpret.
pub fn validate_rel_path(rel: &str) -> Result<()> {
    let bad = |why: &str| Err(CoreError::UnsafePath(format!("{rel:?}: {why}")));
    if rel.is_empty() {
        return bad("empty path");
    }
    if rel.contains('\0') {
        return bad("contains NUL");
    }
    if rel.contains('\\') {
        return bad("contains backslash");
    }
    if rel.starts_with('/') {
        return bad("absolute path");
    }
    for comp in rel.split('/') {
        if comp.is_empty() {
            return bad("empty component");
        }
        if comp == "." || comp == ".." {
            return bad("relative traversal component");
        }
        if cfg!(windows) {
            if comp.contains(':') {
                return bad("drive or alternate data stream separator");
            }
            if comp.ends_with('.') || comp.ends_with(' ') {
                return bad("trailing dot or space is ambiguous on Windows");
            }
            if comp
                .chars()
                .any(|c| matches!(c, '<' | '>' | '"' | '|' | '?' | '*') || (c as u32) < 32)
            {
                return bad("character not allowed on Windows");
            }
            let stem = comp.split('.').next().unwrap_or("").to_ascii_uppercase();
            if WINDOWS_RESERVED.contains(&stem.as_str()) {
                return bad("reserved Windows device name");
            }
        }
    }
    Ok(())
}

/// Join a validated relative path onto `base` component by component.
/// The result is guaranteed to be lexically inside `base`.
pub fn safe_join(base: &Path, rel: &str) -> Result<PathBuf> {
    validate_rel_path(rel)?;
    let mut out = base.to_path_buf();
    for comp in rel.split('/') {
        out.push(comp);
    }
    // Defence in depth: the joined path must still start with base.
    if !out.starts_with(base) {
        return Err(CoreError::UnsafePath(rel.to_string()));
    }
    Ok(out)
}

/// True if the metadata describes a symbolic link, junction or any other
/// reparse point. Such entries are never followed by the scanner.
pub fn is_link_like(meta: &std::fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    false
}

/// Ensure no *existing* ancestor of `base/rel` (strictly below `base`) is a
/// link. Prevents writes from escaping through a planted symlink/junction.
pub fn ensure_no_link_ancestors(base: &Path, rel: &str) -> Result<()> {
    validate_rel_path(rel)?;
    let comps: Vec<&str> = rel.split('/').collect();
    let mut cur = base.to_path_buf();
    for comp in &comps[..comps.len().saturating_sub(1)] {
        cur.push(comp);
        match std::fs::symlink_metadata(&cur) {
            Ok(meta) => {
                if is_link_like(&meta) {
                    return Err(CoreError::UnsafePath(format!(
                        "{} traverses a symbolic link or junction",
                        rel
                    )));
                }
                if !meta.is_dir() {
                    return Err(CoreError::UnsafePath(format!(
                        "{} has a non-directory ancestor",
                        rel
                    )));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(CoreError::io(&cur, e)),
        }
    }
    Ok(())
}

/// Canonicalize a directory for internal use (verbatim/long-path form on
/// Windows) while keeping a simplified version for display and Git.
pub fn canonical_dir(p: &Path) -> Result<PathBuf> {
    std::fs::canonicalize(p).map_err(|e| CoreError::io(p, e))
}

/// Human/Git friendly form of a path (strips `\\?\` where safe).
pub fn display(p: &Path) -> String {
    dunce::simplified(p).to_string_lossy().into_owned()
}

/// Lexically normalise a path (resolve `.` and `..` without touching disk).
pub fn lexical_normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Case-insensitive (Windows) / case-sensitive (elsewhere) "is inside" test on
/// lexically normalised, simplified paths.
pub fn is_within(child: &Path, parent: &Path) -> bool {
    let c = lexical_normalize(dunce::simplified(child));
    let p = lexical_normalize(dunce::simplified(parent));
    if cfg!(windows) {
        let cs = c.to_string_lossy().to_lowercase().replace('/', "\\");
        let ps = p.to_string_lossy().to_lowercase().replace('/', "\\");
        let ps = ps.trim_end_matches('\\').to_string();
        cs == ps || cs.starts_with(&(ps + "\\"))
    } else {
        c.starts_with(&p)
    }
}

/// Compare two paths for equality with platform case rules.
pub fn same_path(a: &Path, b: &Path) -> bool {
    is_within(a, b) && is_within(b, a)
}

/// Convert a Git (forward slash) relative path to a native relative PathBuf.
pub fn rel_to_native(rel: &str) -> PathBuf {
    rel.split('/').collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal_and_absolute() {
        for bad in [
            "",
            "../x",
            "a/../b",
            "/etc/passwd",
            "a//b",
            "./a",
            "a\\b",
            "a\0b",
        ] {
            assert!(
                validate_rel_path(bad).is_err(),
                "{bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn accepts_unicode_and_spaces() {
        for ok in [
            "src/main.rs",
            "文档/说明 文件.md",
            "a b/c-d_e.txt",
            ".gitignore",
            "x/.env",
        ] {
            validate_rel_path(ok).unwrap();
        }
    }

    #[cfg(windows)]
    #[test]
    fn rejects_windows_ambiguities() {
        for bad in [
            "C:/x",
            "a:b",
            "nul",
            "con.txt",
            "dir/aux.log",
            "trail.",
            "space ",
        ] {
            assert!(
                validate_rel_path(bad).is_err(),
                "{bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn safe_join_stays_inside() {
        let base = Path::new("/base");
        let p = safe_join(base, "a/b.txt").unwrap();
        assert!(p.starts_with(base));
        assert!(safe_join(base, "../b").is_err());
    }

    #[test]
    fn within_checks() {
        assert!(is_within(Path::new("/a/b/c"), Path::new("/a/b")));
        assert!(!is_within(Path::new("/a/bc"), Path::new("/a/b")));
        assert!(is_within(Path::new("/a/b"), Path::new("/a/b/")));
    }
}
