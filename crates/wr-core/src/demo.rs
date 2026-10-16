//! Sandbox demo repository. Generated entirely inside the demo vault's own
//! directory so first-time users can explore snapshots, a lost worktree and
//! recovery without touching their real projects.

use crate::error::{CoreError, Result};
use crate::git::Git;
use crate::paths;
use std::fs;
use std::path::{Path, PathBuf};

pub struct DemoPaths {
    pub sandbox: PathBuf,
    pub main: PathBuf,
    pub agent_cache: PathBuf,
    pub agent_metrics: PathBuf,
    pub agent_docs: PathBuf,
}

const ID: [&str; 4] = [
    "-c",
    "user.name=Worktree Rescue Demo",
    "-c",
    "user.email=demo@worktree-rescue.invalid",
];

fn w(path: &Path, content: &str) -> Result<()> {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p).map_err(|e| CoreError::io(p, e))?;
    }
    fs::write(path, content).map_err(|e| CoreError::io(path, e))
}

fn git_w(git: &Git, sandbox: &Path, cwd: &Path, args: &[&str]) -> Result<()> {
    let mut full: Vec<&str> = ID.to_vec();
    full.extend_from_slice(args);
    git.run_fixture_write(sandbox, cwd, &full)?;
    Ok(())
}

pub fn layout(sandbox: &Path) -> DemoPaths {
    DemoPaths {
        sandbox: sandbox.to_path_buf(),
        main: sandbox.join("orbit-api"),
        agent_cache: sandbox.join("orbit-api.agent-cache"),
        agent_metrics: sandbox.join("orbit-api.agent-metrics"),
        agent_docs: sandbox.join("orbit-api.agent-docs"),
    }
}

/// Build the demo repository with three linked worktrees and realistic
/// uncommitted work. Any previous sandbox is removed first.
pub fn create(git: &Git, sandbox: &Path) -> Result<DemoPaths> {
    if sandbox.exists() {
        fs::remove_dir_all(sandbox).map_err(|e| CoreError::io(sandbox, e))?;
    }
    fs::create_dir_all(sandbox).map_err(|e| CoreError::io(sandbox, e))?;
    let p = layout(sandbox);
    let m = &p.main;
    fs::create_dir_all(m).map_err(|e| CoreError::io(m, e))?;
    git_w(git, sandbox, m, &["init", "-q", "-b", "main"])?;
    git_w(git, sandbox, m, &["config", "core.autocrlf", "false"])?;

    w(
        &m.join("README.md"),
        "# Orbit API\n\nDemo service used by the Worktree Rescue sandbox.\n",
    )?;
    w(&m.join(".gitignore"), "node_modules/\ndist/\n*.log\n")?;
    w(
        &m.join("package.json"),
        "{\n  \"name\": \"orbit-api\",\n  \"version\": \"0.4.0\",\n  \"type\": \"module\"\n}\n",
    )?;
    w(
        &m.join("src/server.ts"),
        "import { routes } from './routes/index.js';\n\nexport function start(port = 8080) {\n  return routes.listen(port);\n}\n",
    )?;
    w(
        &m.join("src/routes/index.ts"),
        "export const routes = {\n  listen(port: number) {\n    console.log(`listening on ${port}`);\n  },\n};\n",
    )?;
    w(
        &m.join("src/lib/cache.ts"),
        "const store = new Map<string, unknown>();\n\nexport function get(key: string) {\n  return store.get(key);\n}\n\nexport function set(key: string, value: unknown) {\n  store.set(key, value);\n}\n",
    )?;
    w(
        &m.join("docs/架构 说明.md"),
        "# 架构说明\n\n路径中包含中文与空格，用于验证编码处理。\n",
    )?;
    git_w(git, sandbox, m, &["add", "-A"])?;
    git_w(
        git,
        sandbox,
        m,
        &["commit", "-q", "-m", "Initial service skeleton"],
    )?;
    w(
        &m.join("src/lib/metrics.ts"),
        "export const counters: Record<string, number> = {};\n",
    )?;
    git_w(git, sandbox, m, &["add", "-A"])?;
    git_w(
        git,
        sandbox,
        m,
        &["commit", "-q", "-m", "Add metrics counters"],
    )?;

    // Linked worktrees, one per "agent".
    for (dir, branch) in [
        (&p.agent_cache, "agent/cache-ttl"),
        (&p.agent_metrics, "agent/metrics-endpoint"),
        (&p.agent_docs, "agent/docs-refresh"),
    ] {
        let d = paths::display(dir);
        git_w(
            git,
            sandbox,
            m,
            &["worktree", "add", "-q", "-b", branch, &d],
        )?;
    }

    // Main worktree: modified, staged, deleted, untracked, ignored.
    w(
        &m.join("README.md"),
        "# Orbit API\n\nDemo service used by the Worktree Rescue sandbox.\n\n## Running\n\n`npm start` launches the server on port 8080.\n",
    )?;
    w(
        &m.join("src/routes/health.ts"),
        "export function health() {\n  return { ok: true, at: Date.now() };\n}\n",
    )?;
    git_w(git, sandbox, m, &["add", "src/routes/health.ts"])?;
    fs::remove_file(m.join("src/lib/metrics.ts")).map_err(|e| CoreError::io(m, e))?;
    w(
        &m.join("notes/todo.md"),
        "- wire health route\n- review agent branches\n",
    )?;
    w(&m.join("server.log"), "ignored log output\n")?;

    // Agent: cache TTL – substantial uncommitted refactor.
    w(
        &p.agent_cache.join("src/lib/cache.ts"),
        "type Entry = { value: unknown; expires: number };\nconst store = new Map<string, Entry>();\n\nexport function get(key: string) {\n  const e = store.get(key);\n  if (!e || e.expires < Date.now()) return undefined;\n  return e.value;\n}\n\nexport function set(key: string, value: unknown, ttlMs = 60_000) {\n  store.set(key, { value, expires: Date.now() + ttlMs });\n}\n",
    )?;
    w(
        &p.agent_cache.join("src/lib/cache.test.ts"),
        "import { get, set } from './cache.js';\n\nset('a', 1, 10);\nconsole.assert(get('a') === 1);\n",
    )?;

    // Agent: metrics endpoint – work that will be "lost" with its directory.
    w(
        &p.agent_metrics.join("src/routes/metrics.ts"),
        "import { counters } from '../lib/metrics.js';\n\nexport function metrics() {\n  return Object.entries(counters)\n    .map(([k, v]) => `${k} ${v}`)\n    .join('\\n');\n}\n",
    )?;
    w(
        &p.agent_metrics.join("src/lib/metrics.ts"),
        "export const counters: Record<string, number> = {};\n\nexport function inc(name: string) {\n  counters[name] = (counters[name] ?? 0) + 1;\n}\n",
    )?;
    git_w(
        git,
        sandbox,
        &p.agent_metrics,
        &["add", "src/lib/metrics.ts"],
    )?;

    // Agent: docs – small edit, directory will be moved.
    w(
        &p.agent_docs.join("docs/架构 说明.md"),
        "# 架构说明\n\n路径中包含中文与空格，用于验证编码处理。\n\n## 缓存\n\n缓存条目带有过期时间。\n",
    )?;
    Ok(p)
}

/// Simulate the two classic accidents: one worktree directory deleted by a
/// cleanup script, another moved by hand. Only touches the sandbox.
pub fn simulate_loss(p: &DemoPaths) -> Result<PathBuf> {
    for dir in [&p.agent_metrics, &p.agent_docs] {
        if !paths::is_within(dir, &p.sandbox) {
            return Err(CoreError::Other("demo path escaped sandbox".into()));
        }
    }
    fs::remove_dir_all(&p.agent_metrics).map_err(|e| CoreError::io(&p.agent_metrics, e))?;
    let moved = p.sandbox.join("orbit-api.docs-moved");
    fs::rename(&p.agent_docs, &moved).map_err(|e| CoreError::io(&p.agent_docs, e))?;
    Ok(moved)
}

/// Latest-state tweak after the first snapshot so the timeline shows change.
pub fn second_edit(p: &DemoPaths) -> Result<()> {
    w(
        &p.agent_cache.join("src/lib/cache.ts"),
        "type Entry = { value: unknown; expires: number };\nconst store = new Map<string, Entry>();\nconst DEFAULT_TTL = 60_000;\n\nexport function get(key: string) {\n  const e = store.get(key);\n  if (!e) return undefined;\n  if (e.expires < Date.now()) {\n    store.delete(key);\n    return undefined;\n  }\n  return e.value;\n}\n\nexport function set(key: string, value: unknown, ttlMs = DEFAULT_TTL) {\n  store.set(key, { value, expires: Date.now() + ttlMs });\n}\n",
    )?;
    w(
        &p.main.join("notes/todo.md"),
        "- wire health route\n- review agent branches\n- merge cache TTL\n",
    )
}
