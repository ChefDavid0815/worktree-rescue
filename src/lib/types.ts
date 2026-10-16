// TypeScript mirror of the Rust DTOs in crates/wr-core/src/types.rs and
// related modules. Field names are camelCase on both sides.

export type Monitoring = "active" | "paused";

export interface RiskCounts {
  info: number;
  attention: number;
  critical: number;
}

export interface Project {
  id: string;
  name: string;
  rootPath: string;
  commonDir: string;
  isDemo: boolean;
  addedAt: number;
  autoProtect: boolean;
  monitoring: Monitoring;
  lastSnapshotAt: number | null;
  lastVerifiedSnapshotAt: number | null;
  snapshotCount: number;
  storedBytes: number;
  openRisks: RiskCounts;
}

export type WorktreeDiagnosis =
  | "healthy"
  | "missing"
  | "possiblyMoved"
  | "prunableCandidate"
  | "volumeUnavailable"
  | "inaccessible"
  | "unknown";

export interface StatusCounts {
  staged: number;
  unstaged: number;
  untracked: number;
  conflicted: number;
  deleted: number;
  trackedPaths: number;
}

export interface Worktree {
  id: string;
  projectId: string;
  path: string;
  isMain: boolean;
  branch: string | null;
  headOid: string | null;
  detached: boolean;
  locked: boolean;
  lockReason: string | null;
  gitPrunable: boolean;
  prunableReason: string | null;
  diagnosis: WorktreeDiagnosis;
  evidence: string[];
  possibleNewPath: string | null;
  dirty: boolean | null;
  counts: StatusCounts | null;
  firstSeenAt: number;
  lastSeenAt: number | null;
  lastCheckedAt: number;
  /** Last time the uncommitted state changed (fingerprint changed). */
  lastDirtyAt: number | null;
  dirtyFingerprint: string | null;
  lastConfirmedAt: number | null;
  /** Latest moment the worktree state is known to be captured. */
  lastProtectedAt: number | null;
  lastSnapshotId: string | null;
  lastSnapshotAt: number | null;
  lastVerifiedSnapshotAt: number | null;
  goneFromGit: boolean;
}

export type SnapshotTrigger = "manual" | "auto" | "checkpoint" | "preRestore";
export type SnapshotStatus = "inProgress" | "verified" | "partial" | "failed" | "interrupted";

export interface SnapshotSummary {
  id: string;
  projectId: string;
  worktreePath: string;
  createdAt: number;
  finishedAt: number | null;
  trigger: SnapshotTrigger;
  status: SnapshotStatus;
  branch: string | null;
  headOid: string | null;
  detached: boolean;
  entryCount: number;
  capturedCount: number;
  skippedCount: number;
  failedCount: number;
  stagedCount: number;
  unstagedCount: number;
  untrackedCount: number;
  deletedCount: number;
  protectedBytes: number;
  newBytes: number;
  manifestSha256: string | null;
  pinned: boolean;
  note: string | null;
  errorCode: string | null;
  errorMessage: string | null;
  verifiedAt: number | null;
}

export interface SnapshotFile {
  path: string;
  origPath: string | null;
  category: string;
  x: string;
  y: string;
  kind: string;
  wtState: "present" | "absent" | "skipped" | "failed";
  wtBlob: string | null;
  wtSize: number | null;
  indexBlob: string | null;
  indexOid: string | null;
  headOid: string | null;
  skipReason: string | null;
  linkTarget: string | null;
  lfsPointer: boolean;
  binary: boolean;
}

export interface GitContext {
  branch: string | null;
  headOid: string | null;
  detached: boolean;
  unborn: boolean;
  upstream: string | null;
  gitDir: string;
  commonDir: string;
  sparseCheckout: boolean;
}

export interface SnapshotDetail {
  summary: SnapshotSummary;
  files: SnapshotFile[];
  git: GitContext | null;
  ignoredNotProtected: number;
  manifestOk: boolean;
  manifestError: string | null;
}

export interface VerifyResult {
  snapshotId: string;
  ok: boolean;
  manifestOk: boolean;
  blobsChecked: number;
  problems: [string, string][];
  checkedAt: number;
}

export interface CaptureOutcome {
  snapshot: SnapshotSummary | null;
  unchanged: boolean;
}

export type Severity = "info" | "attention" | "critical";
export type Confidence = "low" | "medium" | "high";

export interface Risk {
  id: string;
  projectId: string;
  worktreePath: string | null;
  rule: string;
  severity: Severity;
  confidence: Confidence;
  detectedAt: number;
  lastSeenAt: number;
  resolvedAt: number | null;
  acknowledged: boolean;
  evidence: Record<string, unknown>;
}

export interface TimelineEvent {
  id: number;
  projectId: string;
  worktreePath: string | null;
  at: number;
  kind: string;
  snapshotId: string | null;
  data: Record<string, unknown>;
}

export interface EventQuery {
  projectId: string;
  worktreePath?: string | null;
  kinds?: string[] | null;
  since?: number | null;
  until?: number | null;
  limit?: number | null;
}

export interface SnapshotQuery {
  projectId: string;
  worktreePath?: string | null;
  since?: number | null;
  until?: number | null;
  pathContains?: string | null;
  limit?: number | null;
}

export interface FileVersion {
  snapshotId: string;
  createdAt: number;
  worktreePath: string;
  branch: string | null;
  status: SnapshotStatus;
  file: SnapshotFile;
}

export interface BlobContent {
  sha256: string | null;
  size: number;
  binary: boolean;
  truncated: boolean;
  text: string | null;
  source: string;
}

export interface Progress {
  taskId: string;
  phase: string;
  done: number;
  total: number;
  current: string | null;
}

export interface ProjectStorage {
  projectId: string;
  name: string;
  snapshotCount: number;
  referencedBytes: number;
  budgetBytes: number;
}

export interface StorageStats {
  dataDir: string;
  blobCount: number;
  blobBytes: number;
  manifestCount: number;
  snapshotCount: number;
  availableBytes: number | null;
  perProject: ProjectStorage[];
}

export interface StorageOverview {
  main: StorageStats;
  demo: StorageStats;
}

export interface GcReport {
  snapshotsPruned: number;
  blobsDeleted: number;
  bytesFreed: number;
  orphanFilesDeleted: number;
  tempFilesDeleted: number;
}

export interface RetentionPolicy {
  keepAllHours: number;
  keepHourlyDays: number;
  keepDailyDays: number;
  maxSnapshotsPerWorktree: number;
}

export interface Settings {
  autoProtectNewProjects: boolean;
  quietPeriodSecs: number;
  minIntervalSecs: number;
  checkpointIntervalSecs: number;
  maxFileBytes: number;
  repoBudgetBytes: number;
  includeUntracked: boolean;
  excludeGlobs: string[];
  sensitiveGlobs: string[];
  includeSensitive: boolean;
  includeIgnoredGlobs: string[];
  recoveryDir: string;
  retention: RetentionPolicy;
  notifyOnRisk: boolean;
  notifyOnFailure: boolean;
  theme: "system" | "dark" | "light";
  language: "auto" | "en" | "zh-CN";
  gitPath: string | null;
  staleProtectionMinutes: number;
  massDeletionThreshold: number;
  minFreeSpaceBytes: number;
  inventoryPollSecs: number;
}

export interface EnvironmentReport {
  os: string;
  appVersion: string;
  gitFound: boolean;
  gitPath: string | null;
  gitVersion: string | null;
  gitSupported: boolean;
  dataDir: string;
  dataDirWritable: boolean;
  availableBytes: number | null;
  recoveryDir: string;
  checkedAt: number;
}

export interface AppInfo {
  environment: EnvironmentReport;
  onboarded: boolean;
  demoDataDir: string;
  logPath: string;
  privacy: string[];
}

export interface RepoIdentity {
  toplevel: string;
  gitDir: string;
  commonDir: string;
  isLinkedWorktree: boolean;
  gitFilePointer: boolean;
  branch: string | null;
  headOid: string | null;
  detached: boolean;
  unborn: boolean;
  sparseCheckout: boolean;
  hasSubmodules: boolean;
  usesLfs: boolean;
}

export interface SupportNote {
  feature: string;
  level: "supported" | "partial" | "unsupported";
}

export interface RepoProbe {
  identity: RepoIdentity;
  mainWorktree: string;
  alreadyRegistered: string | null;
  worktreeCount: number;
  status: StatusCounts | null;
  support: SupportNote[];
}

export interface RefreshResult {
  worktrees: Worktree[];
  newRisks: Risk[];
  resolvedRisks: Risk[];
  events: string[];
}

export interface ItemResult {
  path: string;
  outcome: "written" | "skipped" | "failed" | "listed";
  detail: string | null;
  variant: string | null;
  expectedSha256: string | null;
  writtenSha256: string | null;
  bytes: number | null;
  destination: string | null;
  provenance: string;
}

export interface PhaseRecord {
  phase: string;
  ok: boolean;
  detail: string;
  at: number;
}

export interface RecoveryReport {
  id: string;
  mode: "separateDirectory" | "restoreSelectedFiles";
  source: string;
  provenance: string;
  snapshotId: string | null;
  target: string;
  startedAt: number;
  finishedAt: number;
  status: "completed" | "completedWithIssues" | "aborted" | "failed";
  phases: PhaseRecord[];
  items: ItemResult[];
  written: number;
  skipped: number;
  failed: number;
  bytesWritten: number;
  protectionSnapshotId: string | null;
  gitContext: unknown;
  notes: string[];
}

export type Variant = "workingTree" | "staged";

export interface PreviewItem {
  path: string;
  action: "create" | "overwrite" | "identical" | "blocked";
  blockedReason: string | null;
  currentSha256: string | null;
  currentSize: number | null;
  snapshotSha256: string | null;
  snapshotSize: number | null;
}

export interface RestorePreview {
  id: string;
  snapshotId: string;
  worktreePath: string;
  variant: Variant;
  createdAt: number;
  items: PreviewItem[];
  overwriteCount: number;
  createCount: number;
  blockedCount: number;
}

export interface ForensicCandidate {
  provenance: "gitReflog" | "gitStash" | "unreachableCommit" | "unreachableBlob";
  oid: string;
  objectType: string;
  reference: string | null;
  subject: string | null;
  authorTime: number | null;
  size: number | null;
  preview: string | null;
}

export interface ForensicReport {
  scannedAt: number;
  candidates: ForensicCandidate[];
  reflogEntries: number;
  stashEntries: number;
  unreachableCommits: number;
  unreachableBlobs: number;
  notes: string[];
}

export interface RecoveryRecord {
  id: string;
  snapshotId: string | null;
  source: string;
  mode: string;
  target: string;
  startedAt: number;
  finishedAt: number | null;
  status: string;
  report: RecoveryReport | null;
}

export interface ErrorPayload {
  code: string;
  message: string;
}
