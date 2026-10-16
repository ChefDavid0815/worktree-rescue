import { vi } from "vitest";
import * as fx from "./fixtures";
import type { Settings } from "../lib/types";

export const defaultSettings: Settings = {
  autoProtectNewProjects: false,
  quietPeriodSecs: 20,
  minIntervalSecs: 60,
  checkpointIntervalSecs: 600,
  maxFileBytes: 25 * 1024 * 1024,
  repoBudgetBytes: 2 * 1024 ** 3,
  includeUntracked: true,
  excludeGlobs: ["**/node_modules/**"],
  sensitiveGlobs: ["**/.env"],
  includeSensitive: false,
  includeIgnoredGlobs: [],
  recoveryDir: "C:\\Users\\me\\Documents\\Worktree Rescue\\Recovered",
  retention: { keepAllHours: 24, keepHourlyDays: 7, keepDailyDays: 30, maxSnapshotsPerWorktree: 400 },
  notifyOnRisk: true,
  notifyOnFailure: true,
  theme: "dark",
  language: "en",
  gitPath: null,
  staleProtectionMinutes: 120,
  massDeletionThreshold: 25,
  minFreeSpaceBytes: 2 * 1024 ** 3,
  inventoryPollSecs: 60,
};

export type Handlers = Record<string, (args: Record<string, unknown>) => unknown>;

export function baseHandlers(overrides: Handlers = {}): Handlers {
  let settings = { ...defaultSettings };
  return {
    app_info: () => ({
      environment: {
        os: "windows x86_64",
        appVersion: "1.0.0",
        gitFound: true,
        gitPath: "C:\\Program Files\\Git\\cmd\\git.exe",
        gitVersion: "git version 2.54.0.windows.1",
        gitSupported: true,
        dataDir: "C:\\Users\\me\\AppData\\Local\\WorktreeRescue",
        dataDirWritable: true,
        availableBytes: 100 * 1024 ** 3,
        recoveryDir: settings.recoveryDir,
        checkedAt: Date.now(),
      },
      onboarded: true,
      demoDataDir: "C:\\demo",
      logPath: "C:\\log",
      privacy: [],
    }),
    get_settings: () => settings,
    update_settings: ({ settings: s }) => {
      settings = s as Settings;
      return settings;
    },
    default_settings: () => defaultSettings,
    list_projects: () => [fx.project],
    list_worktrees: () => fx.worktrees,
    query_snapshots: () => fx.snapshots,
    query_events: () => fx.events,
    list_risks: () => fx.risks,
    snapshot_detail: () => fx.detail,
    list_recoveries: () => [],
    storage_stats: () => ({
      main: { dataDir: "C:\\data", blobCount: 3, blobBytes: 4096, manifestCount: 2, snapshotCount: 2, availableBytes: 1e11, perProject: [] },
      demo: { dataDir: "C:\\demo", blobCount: 0, blobBytes: 0, manifestCount: 0, snapshotCount: 0, availableBytes: 1e11, perProject: [] },
    }),
    file_history: () => [],
    worktree_guidance: () => [],
    suggest_recovery_target: () => "C:\\Users\\me\\Documents\\Worktree Rescue\\Recovered\\orbit-api-x",
    ...overrides,
  };
}

/** Install a fake Tauri IPC bridge for the duration of a test. */
export function installBackend(handlers: Handlers) {
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  vi.doMock("@tauri-apps/api/core", () => ({
    invoke: async (cmd: string, args: Record<string, unknown> = {}) => {
      calls.push({ cmd, args });
      const h = handlers[cmd];
      if (!h) throw { code: "other", message: `unmocked command ${cmd}` };
      return h(args);
    },
  }));
  vi.doMock("@tauri-apps/api/event", () => ({ listen: async () => () => {} }));
  vi.doMock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ setTheme: async () => {} }) }));
  vi.doMock("@tauri-apps/plugin-dialog", () => ({ open: async () => null, save: async () => null }));
  return calls;
}
