import type { Project, Risk, SnapshotDetail, SnapshotSummary, StatusCounts, TimelineEvent, Worktree } from "../lib/types";

const now = Date.now();
export const MIN = 60_000;

export const project: Project = {
  id: "p1",
  name: "orbit-api",
  rootPath: "C:\\dev\\orbit-api",
  commonDir: "C:\\dev\\orbit-api\\.git",
  isDemo: false,
  addedAt: now - 3 * 86_400_000,
  autoProtect: true,
  monitoring: "active",
  lastSnapshotAt: now - 5 * MIN,
  lastVerifiedSnapshotAt: now - 5 * MIN,
  snapshotCount: 3,
  storedBytes: 4096,
  openRisks: { info: 0, attention: 0, critical: 1 },
};

const counts = (c: Partial<StatusCounts>): StatusCounts => ({
  staged: 0,
  unstaged: 0,
  untracked: 0,
  conflicted: 0,
  deleted: 0,
  trackedPaths: 0,
  ...c,
});

function wt(over: Partial<Worktree>): Worktree {
  return {
    id: Math.random().toString(36),
    projectId: "p1",
    path: "C:\\dev\\orbit-api",
    isMain: false,
    branch: "main",
    headOid: "a".repeat(40),
    detached: false,
    locked: false,
    lockReason: null,
    gitPrunable: false,
    prunableReason: null,
    diagnosis: "healthy",
    evidence: [],
    possibleNewPath: null,
    dirty: false,
    counts: counts({}),
    firstSeenAt: now - 86_400_000,
    lastSeenAt: now,
    lastCheckedAt: now,
    lastDirtyAt: null,
    dirtyFingerprint: null,
    lastConfirmedAt: null,
    lastProtectedAt: null,
    lastSnapshotId: null,
    lastSnapshotAt: null,
    lastVerifiedSnapshotAt: null,
    goneFromGit: false,
    ...over,
  };
}

export const worktrees: Worktree[] = [
  wt({ id: "w-main", isMain: true, dirty: true, counts: counts({ unstaged: 2 }), lastDirtyAt: now - 30 * MIN, lastProtectedAt: now - 5 * MIN, lastSnapshotAt: now - 5 * MIN, lastSnapshotId: "s1" }),
  wt({ id: "w-cache", path: "C:\\dev\\orbit-api.agent-cache", branch: "agent/cache-ttl", dirty: true, counts: counts({ unstaged: 1, untracked: 1 }), lastDirtyAt: now - 2 * MIN, lastProtectedAt: now - 20 * MIN }),
  wt({
    id: "w-gone",
    path: "C:\\dev\\orbit-api.agent-metrics",
    branch: "agent/metrics-endpoint",
    diagnosis: "prunableCandidate",
    evidence: ["gitReportsPrunable", "pathNotFound", "volumeMounted"],
    dirty: true,
    lastDirtyAt: now - 60 * MIN,
    lastProtectedAt: now - 90 * MIN,
  }),
];

export function snapshot(over: Partial<SnapshotSummary> = {}): SnapshotSummary {
  return {
    id: "s1",
    projectId: "p1",
    worktreePath: "C:\\dev\\orbit-api",
    createdAt: now - 5 * MIN,
    finishedAt: now - 5 * MIN,
    trigger: "manual",
    status: "verified",
    branch: "main",
    headOid: "a".repeat(40),
    detached: false,
    entryCount: 2,
    capturedCount: 2,
    skippedCount: 0,
    failedCount: 0,
    stagedCount: 0,
    unstagedCount: 2,
    untrackedCount: 0,
    deletedCount: 0,
    protectedBytes: 2048,
    newBytes: 2048,
    manifestSha256: "b".repeat(64),
    pinned: false,
    note: null,
    errorCode: null,
    errorMessage: null,
    verifiedAt: now - 5 * MIN,
    ...over,
  };
}

export const snapshots: SnapshotSummary[] = [snapshot(), snapshot({ id: "s2", trigger: "auto", createdAt: now - 50 * MIN })];

export const detail: SnapshotDetail = {
  summary: snapshot(),
  git: {
    branch: "main",
    headOid: "a".repeat(40),
    detached: false,
    unborn: false,
    upstream: null,
    gitDir: "C:\\dev\\orbit-api\\.git",
    commonDir: "C:\\dev\\orbit-api\\.git",
    sparseCheckout: false,
  },
  ignoredNotProtected: 1,
  manifestOk: true,
  manifestError: null,
  files: [
    {
      path: "src/lib/cache.ts",
      origPath: null,
      category: "modified",
      x: ".",
      y: "M",
      kind: "file",
      wtState: "present",
      wtBlob: "c".repeat(64),
      wtSize: 300,
      indexBlob: null,
      indexOid: null,
      headOid: "d".repeat(40),
      skipReason: null,
      linkTarget: null,
      lfsPointer: false,
      binary: false,
    },
    {
      path: "README.md",
      origPath: null,
      category: "modified",
      x: ".",
      y: "M",
      kind: "file",
      wtState: "present",
      wtBlob: "e".repeat(64),
      wtSize: 120,
      indexBlob: null,
      indexOid: null,
      headOid: "f".repeat(40),
      skipReason: null,
      linkTarget: null,
      lfsPointer: false,
      binary: false,
    },
  ],
};

export const risks: Risk[] = [
  {
    id: "r1",
    projectId: "p1",
    worktreePath: "C:\\dev\\orbit-api.agent-metrics",
    rule: "unprotectedWorkUnreachable",
    severity: "critical",
    confidence: "high",
    detectedAt: now - 10 * MIN,
    lastSeenAt: now,
    resolvedAt: null,
    acknowledged: false,
    evidence: { diagnosis: "prunableCandidate", branch: "agent/metrics-endpoint", lastDirtyAt: now - 60 * MIN },
  },
];

export const events: TimelineEvent[] = [
  { id: 1, projectId: "p1", worktreePath: "C:\\dev\\orbit-api.agent-metrics", at: now - 10 * MIN, kind: "worktreeUnreachable", snapshotId: null, data: { diagnosis: "prunableCandidate" } },
  { id: 2, projectId: "p1", worktreePath: "C:\\dev\\orbit-api", at: now - 40 * MIN, kind: "branchChanged", snapshotId: null, data: { from: "main", to: "feature" } },
];
