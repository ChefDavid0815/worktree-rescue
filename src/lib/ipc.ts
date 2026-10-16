// Typed wrappers over Tauri commands. Every call maps 1:1 to a Rust
// `#[tauri::command]` in src-tauri/src/commands.rs.

import { invoke } from "@tauri-apps/api/core";
import type * as T from "./types";

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export class CommandError extends Error {
  code: string;
  constructor(payload: T.ErrorPayload) {
    super(payload.message);
    this.code = payload.code;
  }
}

async function call<R>(cmd: string, args?: Record<string, unknown>): Promise<R> {
  try {
    return await invoke<R>(cmd, args);
  } catch (e) {
    if (e && typeof e === "object" && "code" in e && "message" in e) {
      throw new CommandError(e as T.ErrorPayload);
    }
    throw new CommandError({ code: "other", message: String(e) });
  }
}

export const newTaskId = () =>
  `t-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;

export const api = {
  appInfo: () => call<T.AppInfo>("app_info"),
  completeOnboarding: () => call<void>("complete_onboarding"),
  getSettings: () => call<T.Settings>("get_settings"),
  updateSettings: (settings: T.Settings) => call<T.Settings>("update_settings", { settings }),
  defaultSettings: () => call<T.Settings>("default_settings"),

  probeRepository: (path: string) => call<T.RepoProbe>("probe_repository", { path }),
  addProject: (path: string, autoProtect: boolean) =>
    call<T.Project>("add_project", { path, autoProtect }),
  listProjects: () => call<T.Project[]>("list_projects"),
  removeProject: (projectId: string, deleteBackups: boolean) =>
    call<T.GcReport>("remove_project", { projectId, deleteBackups }),
  setAutoProtect: (projectId: string, enabled: boolean) =>
    call<void>("set_auto_protect", { projectId, enabled }),
  setMonitoring: (projectId: string, active: boolean) =>
    call<void>("set_monitoring", { projectId, active }),
  refreshProject: (projectId: string) => call<T.RefreshResult>("refresh_project", { projectId }),
  listWorktrees: (projectId: string) => call<T.Worktree[]>("list_worktrees", { projectId }),
  worktreeGuidance: (projectId: string, worktreePath: string) =>
    call<string[]>("worktree_guidance", { projectId, worktreePath }),

  createSnapshot: (projectId: string, worktreePath: string, note: string | null, taskId: string) =>
    call<T.CaptureOutcome>("create_snapshot", { projectId, worktreePath, note, taskId }),
  cancelTask: (taskId: string) => call<boolean>("cancel_task", { taskId }),
  querySnapshots: (query: T.SnapshotQuery) => call<T.SnapshotSummary[]>("query_snapshots", { query }),
  snapshotDetail: (snapshotId: string) => call<T.SnapshotDetail>("snapshot_detail", { snapshotId }),
  verifySnapshot: (snapshotId: string, deep: boolean) =>
    call<T.VerifyResult>("verify_snapshot", { snapshotId, deep }),
  setSnapshotPinned: (snapshotId: string, pinned: boolean) =>
    call<void>("set_snapshot_pinned", { snapshotId, pinned }),
  setSnapshotNote: (snapshotId: string, note: string | null) =>
    call<void>("set_snapshot_note", { snapshotId, note }),
  deleteSnapshots: (projectId: string, snapshotIds: string[]) =>
    call<T.GcReport>("delete_snapshots", { projectId, snapshotIds }),
  fileHistory: (projectId: string, path: string) =>
    call<T.FileVersion[]>("file_history", { projectId, path }),
  blobContent: (projectId: string, sha256: string) =>
    call<T.BlobContent>("blob_content", { projectId, sha256 }),
  gitBlobContent: (projectId: string, oid: string) =>
    call<T.BlobContent>("git_blob_content", { projectId, oid }),
  currentFileContent: (projectId: string, worktreePath: string, path: string) =>
    call<T.BlobContent>("current_file_content", { projectId, worktreePath, path }),

  suggestRecoveryTarget: (snapshotId: string) => call<string>("suggest_recovery_target", { snapshotId }),
  recoverToDirectory: (request: {
    snapshotId: string;
    target: string;
    selection: string[] | null;
    includeStaged: boolean;
    includeHeadFiles: boolean;
  }) => call<T.RecoveryReport>("recover_to_directory", { request }),
  previewRestore: (snapshotId: string, paths: string[], variant: T.Variant) =>
    call<T.RestorePreview>("preview_restore", { snapshotId, paths, variant }),
  executeRestore: (projectId: string, previewId: string, taskId: string) =>
    call<T.RecoveryReport>("execute_restore", { projectId, previewId, taskId }),
  listRecoveries: (projectId: string) => call<T.RecoveryRecord[]>("list_recoveries", { projectId }),

  forensicScan: (projectId: string, worktreePath: string | null, deep: boolean, taskId: string) =>
    call<T.ForensicReport>("forensic_scan", { projectId, worktreePath, deep, taskId }),
  forensicCommitFiles: (projectId: string, oid: string) =>
    call<[string, string][]>("forensic_commit_files", { projectId, oid }),
  recoverGitObject: (projectId: string, candidate: T.ForensicCandidate, target: string) =>
    call<T.RecoveryReport>("recover_git_object", { projectId, candidate, target }),
  suggestForensicTarget: (projectId: string, oid: string) =>
    call<string>("suggest_forensic_target", { projectId, oid }),

  listRisks: (projectId: string, includeResolved: boolean) =>
    call<T.Risk[]>("list_risks", { projectId, includeResolved }),
  acknowledgeRisk: (projectId: string, riskId: string) =>
    call<void>("acknowledge_risk", { projectId, riskId }),
  queryEvents: (query: T.EventQuery) => call<T.TimelineEvent[]>("query_events", { query }),

  storageStats: () => call<T.StorageOverview>("storage_stats"),
  retentionPlan: (projectId: string) => call<string[]>("retention_plan", { projectId }),
  runRetention: () => call<T.GcReport>("run_retention"),
  buildDemo: () => call<T.Project>("build_demo"),
  exportDiagnostics: (target: string) => call<string>("export_diagnostics", { target }),
  revealPath: (path: string) => call<void>("reveal_path", { path }),
};

export type Api = typeof api;
