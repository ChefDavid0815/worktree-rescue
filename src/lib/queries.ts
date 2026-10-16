import { QueryClient, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useSyncExternalStore } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, isTauri } from "./ipc";
import type { EventQuery, Progress, SnapshotQuery } from "./types";

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 5_000,
      refetchOnWindowFocus: false,
      retry: 1,
    },
  },
});

export const keys = {
  appInfo: ["appInfo"] as const,
  settings: ["settings"] as const,
  projects: ["projects"] as const,
  worktrees: (p: string) => ["worktrees", p] as const,
  snapshots: (q: SnapshotQuery) => ["snapshots", q.projectId, q] as const,
  snapshotDetail: (id: string) => ["snapshotDetail", id] as const,
  risks: (p: string, all: boolean) => ["risks", p, all] as const,
  events: (q: EventQuery) => ["events", q.projectId, q] as const,
  storage: ["storage"] as const,
  recoveries: (p: string) => ["recoveries", p] as const,
  fileHistory: (p: string, path: string) => ["fileHistory", p, path] as const,
};

export const useAppInfo = () => useQuery({ queryKey: keys.appInfo, queryFn: api.appInfo, staleTime: 60_000 });
export const useSettings = () => useQuery({ queryKey: keys.settings, queryFn: api.getSettings, staleTime: Infinity });
export const useProjects = () => useQuery({ queryKey: keys.projects, queryFn: api.listProjects });

export const useWorktrees = (projectId: string | null) =>
  useQuery({
    queryKey: keys.worktrees(projectId ?? ""),
    queryFn: () => api.listWorktrees(projectId!),
    enabled: !!projectId,
  });

export const useSnapshots = (q: SnapshotQuery | null) =>
  useQuery({
    queryKey: keys.snapshots(q ?? { projectId: "" }),
    queryFn: () => api.querySnapshots(q!),
    enabled: !!q?.projectId,
  });

export const useSnapshotDetail = (id: string | null) =>
  useQuery({
    queryKey: keys.snapshotDetail(id ?? ""),
    queryFn: () => api.snapshotDetail(id!),
    enabled: !!id,
    staleTime: 60_000,
  });

export const useRisks = (projectId: string | null, includeResolved = false) =>
  useQuery({
    queryKey: keys.risks(projectId ?? "", includeResolved),
    queryFn: () => api.listRisks(projectId!, includeResolved),
    enabled: !!projectId,
  });

export const useEvents = (q: EventQuery | null) =>
  useQuery({
    queryKey: keys.events(q ?? { projectId: "" }),
    queryFn: () => api.queryEvents(q!),
    enabled: !!q?.projectId,
  });

export const useStorage = () => useQuery({ queryKey: keys.storage, queryFn: api.storageStats });

export const useRecoveries = (projectId: string | null) =>
  useQuery({
    queryKey: keys.recoveries(projectId ?? ""),
    queryFn: () => api.listRecoveries(projectId!),
    enabled: !!projectId,
  });

export const useFileHistory = (projectId: string | null, path: string | null) =>
  useQuery({
    queryKey: keys.fileHistory(projectId ?? "", path ?? ""),
    queryFn: () => api.fileHistory(projectId!, path!),
    enabled: !!projectId && !!path,
  });

// ---------------------------------------------------------------------------
// Backend events
// ---------------------------------------------------------------------------

type ProgressMap = Record<string, Progress>;
let progressState: ProgressMap = {};
const progressSubs = new Set<() => void>();

function setProgress(p: Progress) {
  progressState = { ...progressState, [p.taskId]: p };
  progressSubs.forEach((f) => f());
}

export function clearProgress(taskId: string) {
  const { [taskId]: _, ...rest } = progressState;
  progressState = rest;
  progressSubs.forEach((f) => f());
}

export function useProgress(taskId: string | null): Progress | null {
  const map = useSyncExternalStore(
    (cb) => {
      progressSubs.add(cb);
      return () => progressSubs.delete(cb);
    },
    () => progressState,
  );
  return taskId ? (map[taskId] ?? null) : null;
}

/** Auto-protection activity: the most recent progress of an automatic task. */
export function useAutoActivity(): Progress | null {
  const map = useSyncExternalStore(
    (cb) => {
      progressSubs.add(cb);
      return () => progressSubs.delete(cb);
    },
    () => progressState,
  );
  const autos = Object.values(map).filter((p) => p.taskId.startsWith("auto-") && p.phase !== "committing");
  return autos[autos.length - 1] ?? null;
}

export function useBackendEvents() {
  const qc = useQueryClient();
  useEffect(() => {
    if (!isTauri()) return;
    const unsubs: Array<() => void> = [];
    let disposed = false;
    listen<{ projectId: string; scope: string }>("wr://changed", (e) => {
      const { projectId, scope } = e.payload;
      qc.invalidateQueries({ queryKey: keys.projects });
      if (projectId) {
        qc.invalidateQueries({ queryKey: ["worktrees", projectId] });
        qc.invalidateQueries({ queryKey: ["risks", projectId] });
        qc.invalidateQueries({ queryKey: ["events", projectId] });
        if (scope === "snapshots" || scope === "recoveries" || scope === "projects") {
          qc.invalidateQueries({ queryKey: ["snapshots", projectId] });
          qc.invalidateQueries({ queryKey: ["fileHistory", projectId] });
          qc.invalidateQueries({ queryKey: ["recoveries", projectId] });
        }
      } else {
        qc.invalidateQueries();
      }
      if (scope === "snapshots" || scope === "storage" || scope === "projects") {
        qc.invalidateQueries({ queryKey: keys.storage });
      }
    }).then((u) => (disposed ? u() : unsubs.push(u)));
    const autoTimers = new Map<string, ReturnType<typeof setTimeout>>();
    listen<Progress>("wr://progress", (e) => {
      const p = e.payload;
      setProgress(p);
      if (p.taskId.startsWith("auto-")) {
        // Automatic captures end silently when nothing changed; expire them.
        clearTimeout(autoTimers.get(p.taskId));
        const delay = p.phase === "committing" ? 1500 : 6000;
        autoTimers.set(
          p.taskId,
          setTimeout(() => {
            autoTimers.delete(p.taskId);
            clearProgress(p.taskId);
          }, delay),
        );
      }
    }).then((u) => (disposed ? u() : unsubs.push(u)));
    return () => {
      disposed = true;
      unsubs.forEach((u) => u());
    };
  }, [qc]);
}
