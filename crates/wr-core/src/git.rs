//! Controlled Git subprocess execution and output parsing.
//!
//! All commands go through [`Git::run`], which only admits an explicit
//! allow-list of read-only subcommands. The few mutating commands needed to
//! build the demo sandbox and test fixtures go through
//! [`Git::run_fixture_write`], which refuses to operate outside a directory
//! that the caller marks as a sandbox.

use crate::error::{CoreError, Result};
use crate::platform;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use wait_timeout::ChildExt;

/// One `cat-file --batch` result: object id and, when found, (type, bytes).
pub type CatFileEntry = (String, Option<(String, Vec<u8>)>);

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// Commands that never modify repository state (with the additional
/// sub-argument checks in [`check_read_only`]).
const READ_ONLY: &[&str] = &[
    "version",
    "rev-parse",
    "status",
    "worktree",
    "cat-file",
    "ls-tree",
    "ls-files",
    "log",
    "reflog",
    "stash",
    "fsck",
    "for-each-ref",
    "symbolic-ref",
    "show-ref",
    "config",
    "diff",
    "count-objects",
    "merge-base",
];

/// Destructive operations that must never be executed by the product.
/// Tests assert that `check_read_only` rejects every one of them.
pub const FORBIDDEN_EXAMPLES: &[&[&str]] = &[
    &["reset", "--hard"],
    &["clean", "-fd"],
    &["checkout", "--", "."],
    &["stash", "push"],
    &["stash"],
    &["stash", "drop"],
    &["worktree", "remove", "--force", "x"],
    &["worktree", "prune"],
    &["worktree", "repair"],
    &["gc", "--prune=now"],
    &["reflog", "expire", "--all"],
    &["reflog", "delete", "HEAD@{0}"],
    &["update-ref", "-d", "HEAD"],
    &["symbolic-ref", "HEAD", "refs/heads/x"],
    &["config", "user.name", "x"],
    &["fsck", "--lost-found"],
    &["restore", "."],
    &["switch", "main"],
];

/// Validate that an argument vector is a read-only Git invocation.
pub fn check_read_only(args: &[&str]) -> Result<()> {
    let deny = |why: &str| {
        Err(CoreError::Other(format!(
            "refusing non read-only git invocation ({why}): git {}",
            args.join(" ")
        )))
    };
    let Some(sub) = args.first() else {
        return deny("empty");
    };
    if !READ_ONLY.contains(sub) {
        return deny("subcommand not allowed");
    }
    let rest = &args[1..];
    match *sub {
        "worktree" => {
            if rest.first() != Some(&"list") {
                return deny("only `worktree list` is allowed");
            }
        }
        "stash" => {
            if !matches!(rest.first(), Some(&"list") | Some(&"show")) {
                return deny("only `stash list|show` is allowed");
            }
        }
        "reflog" => {
            if let Some(first) = rest.first()
                && matches!(*first, "expire" | "delete" | "drop")
            {
                return deny("reflog mutation");
            }
        }
        "config" => {
            if !rest
                .iter()
                .any(|a| matches!(*a, "--get" | "--get-all" | "--get-regexp" | "--list" | "-l"))
            {
                return deny("config must be a query");
            }
        }
        "symbolic-ref" => {
            // `symbolic-ref HEAD <target>` would rewrite HEAD.
            let positional = rest.iter().filter(|a| !a.starts_with('-')).count();
            if positional > 1 || rest.iter().any(|a| *a == "-d" || *a == "--delete") {
                return deny("symbolic-ref mutation");
            }
        }
        "fsck" => {
            if rest.iter().any(|a| a.starts_with("--lost-found")) {
                return deny("fsck --lost-found writes objects");
            }
        }
        "diff"
            if rest
                .iter()
                .any(|a| *a == "--output" || a.starts_with("--output=")) =>
        {
            return deny("diff --output writes files");
        }
        _ => {}
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct Git {
    exe: PathBuf,
    timeout: Duration,
}

#[derive(Default)]
pub struct RunOpts<'a> {
    pub timeout: Option<Duration>,
    pub cancel: Option<&'a AtomicBool>,
    pub stdin: Option<Vec<u8>>,
    /// Exit codes treated as success in addition to 0.
    pub ok_codes: &'a [i32],
}

#[derive(Debug)]
pub struct GitOutput {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl GitOutput {
    pub fn stdout_str(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
}

impl Git {
    pub fn new(exe: PathBuf) -> Self {
        Git {
            exe,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    pub fn discover(override_path: Option<&Path>) -> Result<Self> {
        platform::find_git(override_path)
            .map(Git::new)
            .ok_or(CoreError::GitNotFound)
    }

    pub fn exe(&self) -> &Path {
        &self.exe
    }

    pub fn with_timeout(mut self, t: Duration) -> Self {
        self.timeout = t;
        self
    }

    pub fn version(&self) -> Result<String> {
        let out = self.run(Path::new("."), &["version"])?;
        Ok(String::from_utf8_lossy(&out).trim().to_string())
    }

    /// Run a read-only command and return stdout. Non-zero exit is an error.
    pub fn run(&self, cwd: &Path, args: &[&str]) -> Result<Vec<u8>> {
        Ok(self.run_opts(cwd, args, RunOpts::default())?.stdout)
    }

    pub fn run_opts(&self, cwd: &Path, args: &[&str], opts: RunOpts<'_>) -> Result<GitOutput> {
        check_read_only(args)?;
        self.exec(cwd, args, opts)
    }

    /// Mutating Git command used *only* to build sandbox fixtures (demo
    /// repository, tests). `sandbox_root` must contain `cwd`.
    pub fn run_fixture_write(
        &self,
        sandbox_root: &Path,
        cwd: &Path,
        args: &[&str],
    ) -> Result<Vec<u8>> {
        if !crate::paths::is_within(cwd, sandbox_root) {
            return Err(CoreError::Other(format!(
                "fixture write outside sandbox: {}",
                cwd.display()
            )));
        }
        Ok(self.exec(cwd, args, RunOpts::default())?.stdout)
    }

    fn exec(&self, cwd: &Path, args: &[&str], opts: RunOpts<'_>) -> Result<GitOutput> {
        let command_label = args.iter().take(3).cloned().collect::<Vec<_>>().join(" ");
        let mut cmd = Command::new(&self.exe);
        cmd.arg("--no-pager")
            .args(["-c", "core.quotepath=off"])
            .args(["-c", "color.ui=never"])
            .args(["-c", "core.fsmonitor=false"])
            .args(["-c", "core.safecrlf=false"])
            .args(args)
            .current_dir(dunce::simplified(cwd))
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_PAGER", "cat")
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_OBJECT_DIRECTORY")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(if opts.stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            });
        platform::hide_console(&mut cmd);

        let mut child = cmd.spawn().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                CoreError::GitNotFound
            } else {
                CoreError::io(&self.exe, e)
            }
        })?;

        let stdin_thread = opts.stdin.map(|data| {
            let mut stdin = child.stdin.take().expect("stdin piped");
            std::thread::spawn(move || {
                let _ = stdin.write_all(&data);
            })
        });
        let mut stdout = child.stdout.take().expect("stdout piped");
        let mut stderr = child.stderr.take().expect("stderr piped");
        let out_thread = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stdout.read_to_end(&mut buf);
            buf
        });
        let err_thread = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stderr.read_to_end(&mut buf);
            buf
        });

        let timeout = opts.timeout.unwrap_or(self.timeout);
        let started = Instant::now();
        let status = loop {
            match child
                .wait_timeout(Duration::from_millis(100))
                .map_err(|e| CoreError::io(&self.exe, e))?
            {
                Some(status) => break status,
                None => {
                    if opts.cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(CoreError::Cancelled);
                    }
                    if started.elapsed() >= timeout {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(CoreError::GitTimeout {
                            command: command_label,
                            seconds: timeout.as_secs(),
                        });
                    }
                }
            }
        };
        if let Some(t) = stdin_thread {
            let _ = t.join();
        }
        let stdout = out_thread.join().unwrap_or_default();
        let stderr = err_thread.join().unwrap_or_default();
        let code = status.code().unwrap_or(-1);
        if code != 0 && !opts.ok_codes.contains(&code) {
            return Err(CoreError::GitFailed {
                command: command_label,
                code: status.code(),
                stderr: String::from_utf8_lossy(&stderr)
                    .trim()
                    .chars()
                    .take(600)
                    .collect(),
            });
        }
        Ok(GitOutput {
            code,
            stdout,
            stderr,
        })
    }

    /// Read many objects with a single `git cat-file --batch` process.
    /// Returns `(oid, Option<(type, bytes)>)` in input order.
    pub fn cat_file_batch(
        &self,
        cwd: &Path,
        oids: &[String],
        cancel: Option<&AtomicBool>,
    ) -> Result<Vec<CatFileEntry>> {
        if oids.is_empty() {
            return Ok(vec![]);
        }
        let mut input = Vec::new();
        for oid in oids {
            if !oid.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(CoreError::Other(format!("invalid object id {oid}")));
            }
            input.extend_from_slice(oid.as_bytes());
            input.push(b'\n');
        }
        let out = self.run_opts(
            cwd,
            &["cat-file", "--batch"],
            RunOpts {
                stdin: Some(input),
                cancel,
                timeout: Some(Duration::from_secs(300)),
                ..Default::default()
            },
        )?;
        parse_cat_file_batch(&out.stdout, oids)
    }
}

fn parse_cat_file_batch(data: &[u8], oids: &[String]) -> Result<Vec<CatFileEntry>> {
    let mut pos = 0usize;
    let mut result = Vec::with_capacity(oids.len());
    for oid in oids {
        let nl = data[pos..]
            .iter()
            .position(|b| *b == b'\n')
            .ok_or_else(|| CoreError::Other("truncated cat-file output".into()))?;
        let header = String::from_utf8_lossy(&data[pos..pos + nl]).into_owned();
        pos += nl + 1;
        let parts: Vec<&str> = header.split(' ').collect();
        if parts.len() == 2 && parts[1] == "missing" {
            result.push((oid.clone(), None));
            continue;
        }
        if parts.len() != 3 {
            return Err(CoreError::Other(format!("bad cat-file header {header}")));
        }
        let size: usize = parts[2]
            .parse()
            .map_err(|_| CoreError::Other(format!("bad size in {header}")))?;
        if pos + size > data.len() {
            return Err(CoreError::Other("truncated cat-file content".into()));
        }
        let content = data[pos..pos + size].to_vec();
        pos += size + 1; // trailing LF
        result.push((oid.clone(), Some((parts[1].to_string(), content))));
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// Repository identity
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoIdentity {
    /// Working tree root (simplified, display form).
    pub toplevel: String,
    /// This worktree's git dir (`.git` or `.git/worktrees/<name>`).
    pub git_dir: String,
    /// The shared repository directory.
    pub common_dir: String,
    /// True when this working tree is a linked worktree.
    pub is_linked_worktree: bool,
    /// True when `.git` in the toplevel is a pointer file instead of a dir.
    pub git_file_pointer: bool,
    pub branch: Option<String>,
    pub head_oid: Option<String>,
    pub detached: bool,
    pub unborn: bool,
    pub sparse_checkout: bool,
    pub has_submodules: bool,
    pub uses_lfs: bool,
}

pub fn identify(git: &Git, path: &Path) -> Result<RepoIdentity> {
    if !path.is_dir() {
        return Err(CoreError::NotARepository(path.to_path_buf()));
    }
    let bare = git.run_opts(
        path,
        &["rev-parse", "--is-bare-repository"],
        RunOpts {
            ok_codes: &[128],
            ..Default::default()
        },
    )?;
    if bare.code == 128 {
        return Err(CoreError::NotARepository(path.to_path_buf()));
    }
    if bare.stdout_str().trim() == "true" {
        return Err(CoreError::BareRepository(path.to_path_buf()));
    }
    let out = git.run(
        path,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--show-toplevel",
            "--git-dir",
            "--git-common-dir",
        ],
    )?;
    let text = String::from_utf8_lossy(&out);
    let mut lines = text.lines();
    let toplevel = native(lines.next().unwrap_or_default());
    let git_dir = native(lines.next().unwrap_or_default());
    let common_dir = native(lines.next().unwrap_or_default());
    if toplevel.is_empty() {
        return Err(CoreError::NotARepository(path.to_path_buf()));
    }
    let is_linked = !crate::paths::same_path(Path::new(&git_dir), Path::new(&common_dir));
    let git_file_pointer = Path::new(&toplevel).join(".git").is_file();

    let top = PathBuf::from(&toplevel);
    let sym = git.run_opts(
        &top,
        &["symbolic-ref", "-q", "HEAD"],
        RunOpts {
            ok_codes: &[1],
            ..Default::default()
        },
    )?;
    let branch_ref = sym.stdout_str().trim().to_string();
    let detached = sym.code == 1;
    let head = git.run_opts(
        &top,
        &["rev-parse", "-q", "--verify", "HEAD^{commit}"],
        RunOpts {
            ok_codes: &[1, 128],
            ..Default::default()
        },
    )?;
    let head_oid = if head.code == 0 {
        Some(head.stdout_str().trim().to_string())
    } else {
        None
    };
    let unborn = head_oid.is_none() && !detached;

    let sparse = config_bool(git, &top, "core.sparseCheckout");
    let has_submodules = top.join(".gitmodules").is_file();
    let uses_lfs = std::fs::read_to_string(top.join(".gitattributes"))
        .map(|s| s.contains("filter=lfs"))
        .unwrap_or(false);

    Ok(RepoIdentity {
        toplevel,
        git_dir,
        common_dir,
        is_linked_worktree: is_linked,
        git_file_pointer,
        branch: branch_ref
            .strip_prefix("refs/heads/")
            .map(str::to_string)
            .or(if branch_ref.is_empty() {
                None
            } else {
                Some(branch_ref.clone())
            }),
        head_oid,
        detached,
        unborn,
        sparse_checkout: sparse,
        has_submodules,
        uses_lfs,
    })
}

fn config_bool(git: &Git, cwd: &Path, key: &str) -> bool {
    git.run_opts(
        cwd,
        &["config", "--get", "--type=bool", key],
        RunOpts {
            ok_codes: &[1],
            ..Default::default()
        },
    )
    .map(|o| o.stdout_str().trim() == "true")
    .unwrap_or(false)
}

/// Convert Git's forward-slash absolute paths into native form.
pub fn native(p: &str) -> String {
    if cfg!(windows) {
        p.trim().replace('/', "\\")
    } else {
        p.trim().to_string()
    }
}

// ---------------------------------------------------------------------------
// Status (porcelain v2)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StatusKind {
    Ordinary,
    Renamed,
    Unmerged,
    Untracked,
    Ignored,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusEntry {
    pub kind: StatusKind,
    /// Index status letter ('.' when unchanged).
    pub x: char,
    /// Worktree status letter ('.' when unchanged).
    pub y: char,
    pub submodule: bool,
    pub mode_head: u32,
    pub mode_index: u32,
    pub mode_worktree: u32,
    pub oid_head: Option<String>,
    pub oid_index: Option<String>,
    pub path: String,
    pub orig_path: Option<String>,
}

impl StatusEntry {
    pub fn staged(&self) -> bool {
        matches!(self.kind, StatusKind::Ordinary | StatusKind::Renamed) && self.x != '.'
    }
    pub fn unstaged(&self) -> bool {
        matches!(self.kind, StatusKind::Ordinary | StatusKind::Renamed) && self.y != '.'
    }
    pub fn is_dir_entry(&self) -> bool {
        self.path.ends_with('/')
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusReport {
    pub branch: Option<String>,
    pub head_oid: Option<String>,
    pub upstream: Option<String>,
    pub ahead: Option<i64>,
    pub behind: Option<i64>,
    pub entries: Vec<StatusEntry>,
}

impl StatusReport {
    pub fn staged_count(&self) -> usize {
        self.entries.iter().filter(|e| e.staged()).count()
    }
    pub fn unstaged_count(&self) -> usize {
        self.entries.iter().filter(|e| e.unstaged()).count()
    }
    pub fn untracked_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.kind == StatusKind::Untracked)
            .count()
    }
    pub fn conflicted_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.kind == StatusKind::Unmerged)
            .count()
    }
    pub fn ignored(&self) -> impl Iterator<Item = &StatusEntry> {
        self.entries
            .iter()
            .filter(|e| e.kind == StatusKind::Ignored)
    }
    pub fn is_dirty(&self) -> bool {
        self.entries.iter().any(|e| e.kind != StatusKind::Ignored)
    }
}

pub fn status(
    git: &Git,
    worktree: &Path,
    include_ignored: bool,
    cancel: Option<&AtomicBool>,
) -> Result<StatusReport> {
    let mut args = vec![
        "status",
        "--porcelain=v2",
        "-z",
        "--branch",
        "--untracked-files=all",
    ];
    if include_ignored {
        args.push("--ignored=matching");
    }
    let out = git.run_opts(
        worktree,
        &args,
        RunOpts {
            cancel,
            timeout: Some(Duration::from_secs(120)),
            ..Default::default()
        },
    )?;
    parse_status_v2(&out.stdout)
}

fn parse_mode(s: &str) -> u32 {
    u32::from_str_radix(s, 8).unwrap_or(0)
}

fn oid_opt(s: &str) -> Option<String> {
    if s.bytes().all(|b| b == b'0') {
        None
    } else {
        Some(s.to_string())
    }
}

pub fn parse_status_v2(data: &[u8]) -> Result<StatusReport> {
    let mut report = StatusReport::default();
    let mut records = data.split(|b| *b == 0).peekable();
    while let Some(raw) = records.next() {
        if raw.is_empty() {
            continue;
        }
        let line = std::str::from_utf8(raw)
            .map_err(|_| CoreError::Other("status output is not UTF-8".into()))?;
        if let Some(h) = line.strip_prefix("# ") {
            if let Some(v) = h.strip_prefix("branch.oid ") {
                report.head_oid = if v == "(initial)" {
                    None
                } else {
                    Some(v.to_string())
                };
            } else if let Some(v) = h.strip_prefix("branch.head ") {
                report.branch = if v == "(detached)" {
                    None
                } else {
                    Some(v.to_string())
                };
            } else if let Some(v) = h.strip_prefix("branch.upstream ") {
                report.upstream = Some(v.to_string());
            } else if let Some(v) = h.strip_prefix("branch.ab ") {
                let mut it = v.split(' ');
                report.ahead = it
                    .next()
                    .and_then(|a| a.trim_start_matches('+').parse().ok());
                report.behind = it
                    .next()
                    .and_then(|b| b.trim_start_matches('-').parse().ok());
            }
            continue;
        }
        let (tag, rest) = line.split_at(1);
        let rest = rest.strip_prefix(' ').unwrap_or(rest);
        match tag {
            "1" => {
                // XY sub mH mI mW hH hI path
                let f: Vec<&str> = rest.splitn(8, ' ').collect();
                if f.len() < 8 {
                    return Err(CoreError::Other(format!("bad status line {line}")));
                }
                let xy: Vec<char> = f[0].chars().collect();
                report.entries.push(StatusEntry {
                    kind: StatusKind::Ordinary,
                    x: xy[0],
                    y: xy[1],
                    submodule: f[1].starts_with('S'),
                    mode_head: parse_mode(f[2]),
                    mode_index: parse_mode(f[3]),
                    mode_worktree: parse_mode(f[4]),
                    oid_head: oid_opt(f[5]),
                    oid_index: oid_opt(f[6]),
                    path: f[7].to_string(),
                    orig_path: None,
                });
            }
            "2" => {
                // XY sub mH mI mW hH hI Xscore path \0 origPath
                let f: Vec<&str> = rest.splitn(9, ' ').collect();
                if f.len() < 9 {
                    return Err(CoreError::Other(format!("bad status line {line}")));
                }
                let orig = records
                    .next()
                    .map(|o| String::from_utf8_lossy(o).into_owned());
                let xy: Vec<char> = f[0].chars().collect();
                report.entries.push(StatusEntry {
                    kind: StatusKind::Renamed,
                    x: xy[0],
                    y: xy[1],
                    submodule: f[1].starts_with('S'),
                    mode_head: parse_mode(f[2]),
                    mode_index: parse_mode(f[3]),
                    mode_worktree: parse_mode(f[4]),
                    oid_head: oid_opt(f[5]),
                    oid_index: oid_opt(f[6]),
                    path: f[8].to_string(),
                    orig_path: orig,
                });
            }
            "u" => {
                // XY sub m1 m2 m3 mW h1 h2 h3 path
                let f: Vec<&str> = rest.splitn(10, ' ').collect();
                if f.len() < 10 {
                    return Err(CoreError::Other(format!("bad status line {line}")));
                }
                let xy: Vec<char> = f[0].chars().collect();
                report.entries.push(StatusEntry {
                    kind: StatusKind::Unmerged,
                    x: xy[0],
                    y: xy[1],
                    submodule: f[1].starts_with('S'),
                    mode_head: parse_mode(f[2]),
                    mode_index: parse_mode(f[3]),
                    mode_worktree: parse_mode(f[5]),
                    oid_head: oid_opt(f[6]),
                    oid_index: oid_opt(f[7]),
                    path: f[9].to_string(),
                    orig_path: None,
                });
            }
            "?" | "!" => {
                report.entries.push(StatusEntry {
                    kind: if tag == "?" {
                        StatusKind::Untracked
                    } else {
                        StatusKind::Ignored
                    },
                    x: '?',
                    y: '?',
                    submodule: false,
                    mode_head: 0,
                    mode_index: 0,
                    mode_worktree: 0,
                    oid_head: None,
                    oid_index: None,
                    path: rest.to_string(),
                    orig_path: None,
                });
            }
            _ => {}
        }
    }
    Ok(report)
}

// ---------------------------------------------------------------------------
// Worktree list (porcelain -z)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeRecord {
    pub path: String,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub bare: bool,
    pub detached: bool,
    pub locked: bool,
    pub lock_reason: Option<String>,
    pub prunable: bool,
    pub prunable_reason: Option<String>,
}

pub fn worktree_list(git: &Git, cwd: &Path) -> Result<Vec<WorktreeRecord>> {
    let out = git.run(cwd, &["worktree", "list", "--porcelain", "-z"])?;
    Ok(parse_worktree_list(&out))
}

pub fn parse_worktree_list(data: &[u8]) -> Vec<WorktreeRecord> {
    let mut list = Vec::new();
    let mut cur: Option<WorktreeRecord> = None;
    for raw in data.split(|b| *b == 0) {
        let line = String::from_utf8_lossy(raw);
        if line.is_empty() {
            if let Some(w) = cur.take() {
                list.push(w);
            }
            continue;
        }
        let (key, val) = match line.split_once(' ') {
            Some((k, v)) => (k.to_string(), Some(v.to_string())),
            None => (line.to_string(), None),
        };
        match key.as_str() {
            "worktree" => {
                if let Some(w) = cur.take() {
                    list.push(w);
                }
                cur = Some(WorktreeRecord {
                    path: native(&val.unwrap_or_default()),
                    ..Default::default()
                });
            }
            _ => {
                let Some(w) = cur.as_mut() else { continue };
                match key.as_str() {
                    "HEAD" => w.head = val,
                    "branch" => {
                        w.branch =
                            val.map(|b| b.strip_prefix("refs/heads/").unwrap_or(&b).to_string())
                    }
                    "bare" => w.bare = true,
                    "detached" => w.detached = true,
                    "locked" => {
                        w.locked = true;
                        w.lock_reason = val.filter(|v| !v.is_empty());
                    }
                    "prunable" => {
                        w.prunable = true;
                        w.prunable_reason = val.filter(|v| !v.is_empty());
                    }
                    _ => {}
                }
            }
        }
    }
    if let Some(w) = cur.take() {
        list.push(w);
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forbids_destructive_commands() {
        for args in FORBIDDEN_EXAMPLES {
            assert!(check_read_only(args).is_err(), "should forbid {args:?}");
        }
        for ok in [
            &["status", "--porcelain=v2"][..],
            &["worktree", "list", "--porcelain"],
            &["stash", "list"],
            &["config", "--get", "core.bare"],
            &["symbolic-ref", "-q", "HEAD"],
            &["fsck", "--unreachable", "--no-reflogs"],
            &["reflog", "show", "--format=%H"],
        ] {
            check_read_only(ok).unwrap();
        }
    }

    #[test]
    fn parses_status_v2() {
        let data = b"# branch.oid abc123\0# branch.head main\0\
1 .M N... 100644 100644 100644 aaaa bbbb src/a.rs\0\
1 A. N... 000000 100644 100644 0000000000 cccc new file.txt\0\
2 R. N... 100644 100644 100644 dddd dddd R100 dst.txt\0src.txt\0\
? untracked/\xe4\xb8\xad.txt\0! node_modules/\0";
        let r = parse_status_v2(data).unwrap();
        assert_eq!(r.branch.as_deref(), Some("main"));
        assert_eq!(r.entries.len(), 5);
        assert!(r.entries[0].unstaged() && !r.entries[0].staged());
        assert!(r.entries[1].staged());
        assert_eq!(r.entries[1].path, "new file.txt");
        assert_eq!(r.entries[2].orig_path.as_deref(), Some("src.txt"));
        assert_eq!(r.entries[3].path, "untracked/中.txt");
        assert!(r.entries[4].is_dir_entry());
    }

    #[test]
    fn parses_worktree_list() {
        let data = b"worktree /r/main\0HEAD 1111\0branch refs/heads/main\0\0\
worktree /r/feat\0HEAD 2222\0detached\0locked agent busy\0\0\
worktree /r/gone\0HEAD 3333\0branch refs/heads/x\0prunable gitdir file points to non-existent location\0\0";
        let l = parse_worktree_list(data);
        assert_eq!(l.len(), 3);
        assert_eq!(l[0].branch.as_deref(), Some("main"));
        assert!(l[1].detached && l[1].locked);
        assert_eq!(l[1].lock_reason.as_deref(), Some("agent busy"));
        assert!(l[2].prunable);
    }

    #[test]
    fn parses_cat_file() {
        let data = b"aa blob 3\nabc\nbb missing\n";
        let r = parse_cat_file_batch(data, &["aa".into(), "bb".into()]).unwrap();
        assert_eq!(r[0].1.as_ref().unwrap().1, b"abc");
        assert!(r[1].1.is_none());
    }
}
