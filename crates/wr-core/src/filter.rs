//! Capture policy: decides which paths the snapshot engine protects and,
//! just as importantly, records *why* a path was left out so the UI can be
//! precise about coverage.

use crate::error::{CoreError, Result};
use crate::settings::Settings;
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SkipReason {
    /// Untracked capture disabled in settings.
    UntrackedDisabled,
    /// Matched a default/user exclusion (dependency or build cache).
    Excluded,
    /// Matched a sensitive-file pattern and sensitive capture is off.
    Sensitive,
    /// Larger than the per-file limit.
    TooLarge,
    /// Kept changing while being read.
    Unstable,
    /// Could not be read (locked, permission denied…).
    Unreadable,
    /// Git-ignored and not covered by an include rule.
    GitIgnored,
    /// Path is not representable safely (traversal, reserved names…).
    UnsafePath,
    /// Submodule contents are not captured in V1.
    Submodule,
    /// Not a regular file or symlink (device, socket…).
    UnsupportedType,
    /// Repository storage budget would be exceeded.
    BudgetExceeded,
}

impl SkipReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            SkipReason::UntrackedDisabled => "untrackedDisabled",
            SkipReason::Excluded => "excluded",
            SkipReason::Sensitive => "sensitive",
            SkipReason::TooLarge => "tooLarge",
            SkipReason::Unstable => "unstable",
            SkipReason::Unreadable => "unreadable",
            SkipReason::GitIgnored => "gitIgnored",
            SkipReason::UnsafePath => "unsafePath",
            SkipReason::Submodule => "submodule",
            SkipReason::UnsupportedType => "unsupportedType",
            SkipReason::BudgetExceeded => "budgetExceeded",
        }
    }
}

pub struct CapturePolicy {
    pub max_file_bytes: u64,
    pub include_untracked: bool,
    pub include_sensitive: bool,
    exclude: GlobSet,
    sensitive: GlobSet,
    include_ignored: GlobSet,
    /// Literal directory prefixes of include rules (text before the first
    /// glob metacharacter), used to decide which ignored directories to walk.
    include_prefixes: Vec<String>,
    pub has_ignored_includes: bool,
}

fn literal_prefix(glob: &str) -> String {
    let cut = glob.find(['*', '?', '[', '{']).unwrap_or(glob.len());
    glob[..cut].trim_end_matches('/').to_string()
}

fn build(globs: &[String]) -> Result<GlobSet> {
    let mut b = GlobSetBuilder::new();
    for g in globs {
        let glob = Glob::new(g).map_err(|e| CoreError::InvalidSetting(format!("{g}: {e}")))?;
        b.add(glob);
        // Make "**/dir/**" also match the directory entry itself ("dir/").
        if let Some(stripped) = g.strip_suffix("/**")
            && let Ok(glob) = Glob::new(stripped)
        {
            b.add(glob);
        }
    }
    b.build()
        .map_err(|e| CoreError::InvalidSetting(e.to_string()))
}

impl CapturePolicy {
    pub fn from_settings(s: &Settings) -> Result<Self> {
        Ok(CapturePolicy {
            max_file_bytes: s.max_file_bytes.max(0) as u64,
            include_untracked: s.include_untracked,
            include_sensitive: s.include_sensitive,
            exclude: build(&s.exclude_globs)?,
            sensitive: build(&s.sensitive_globs)?,
            include_ignored: build(&s.include_ignored_globs)?,
            include_prefixes: s
                .include_ignored_globs
                .iter()
                .map(|g| literal_prefix(g))
                .filter(|p| !p.is_empty())
                .collect(),
            has_ignored_includes: !s.include_ignored_globs.is_empty(),
        })
    }

    fn norm(path: &str) -> &str {
        path.trim_end_matches('/')
    }

    pub fn is_sensitive(&self, path: &str) -> bool {
        self.sensitive.is_match(Self::norm(path))
    }

    pub fn is_excluded(&self, path: &str) -> bool {
        self.exclude.is_match(Self::norm(path))
    }

    /// Decide for an untracked (not ignored) path.
    pub fn untracked_decision(&self, path: &str) -> Option<SkipReason> {
        if !self.include_untracked {
            return Some(SkipReason::UntrackedDisabled);
        }
        if self.is_excluded(path) {
            return Some(SkipReason::Excluded);
        }
        if !self.include_sensitive && self.is_sensitive(path) {
            return Some(SkipReason::Sensitive);
        }
        None
    }

    /// Decide for a Git-ignored path: captured only on explicit include.
    pub fn ignored_included(&self, path: &str) -> bool {
        self.include_ignored.is_match(Self::norm(path))
    }

    /// Should an ignored *directory* be walked looking for included files?
    /// True when an include rule matches it or its literal prefix lies
    /// inside / above it (`target/keep/**` walks `target/`).
    pub fn walk_ignored_dir(&self, dir: &str) -> bool {
        let d = Self::norm(dir);
        if self.include_ignored.is_match(d) {
            return true;
        }
        self.include_prefixes
            .iter()
            .any(|p| p == d || p.starts_with(&format!("{d}/")) || d.starts_with(&format!("{p}/")))
    }

    /// Tracked files are always captured (subject to size) – even sensitive
    /// ones, since they are already in Git history. Sensitive filtering only
    /// applies to files Git does not know about.
    pub fn size_decision(&self, size: u64) -> Option<SkipReason> {
        if size > self.max_file_bytes {
            Some(SkipReason::TooLarge)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy() {
        let p = CapturePolicy::from_settings(&Settings::default()).unwrap();
        assert_eq!(p.untracked_decision("src/new.rs"), None);
        assert_eq!(
            p.untracked_decision("web/node_modules/x/index.js"),
            Some(SkipReason::Excluded)
        );
        assert_eq!(
            p.untracked_decision("node_modules/"),
            Some(SkipReason::Excluded)
        );
        assert_eq!(p.untracked_decision(".env"), Some(SkipReason::Sensitive));
        assert_eq!(
            p.untracked_decision("api/.env.local"),
            Some(SkipReason::Sensitive)
        );
        assert_eq!(
            p.size_decision(26 * 1024 * 1024),
            Some(SkipReason::TooLarge)
        );
    }

    #[test]
    fn opt_in_sensitive_and_ignored() {
        let s = Settings {
            include_sensitive: true,
            include_ignored_globs: vec!["**/notes/**".into()],
            ..Settings::default()
        };
        let p = CapturePolicy::from_settings(&s).unwrap();
        assert_eq!(p.untracked_decision(".env"), None);
        assert!(p.ignored_included("docs/notes/a.md"));
        assert!(p.ignored_included("notes/"));
        assert!(!p.ignored_included("build/out.txt"));
        let s2 = Settings {
            include_ignored_globs: vec!["target/keep/**".into()],
            ..Settings::default()
        };
        let p2 = CapturePolicy::from_settings(&s2).unwrap();
        assert!(p2.walk_ignored_dir("target/"));
        assert!(!p2.walk_ignored_dir("dist/"));
        assert!(p2.ignored_included("target/keep/a.md"));
    }
}
