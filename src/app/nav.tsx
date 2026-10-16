import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react";

export type View = "overview" | "worktrees" | "timeline" | "recovery" | "risks" | "settings";

export type Selection =
  | { type: "worktree"; path: string }
  | { type: "snapshot"; id: string }
  | { type: "risk"; id: string }
  | { type: "event"; id: number }
  | null;

/** Context handed to the Recovery workspace. */
export interface RecoveryIntent {
  snapshotId: string;
  paths?: string[];
  mode?: "separateDirectory" | "restoreSelectedFiles";
}

interface NavState {
  view: View;
  projectId: string | null;
  selection: Selection;
  inspectorOpen: boolean;
  recovery: RecoveryIntent | null;
  /** Timeline filter seeded by other views (e.g. "show this worktree"). */
  timelineWorktree: string | null;
}

interface NavValue extends NavState {
  go: (view: View, opts?: { selection?: Selection; timelineWorktree?: string | null }) => void;
  select: (s: Selection) => void;
  setProject: (id: string | null) => void;
  setInspectorOpen: (open: boolean) => void;
  startRecovery: (intent: RecoveryIntent | null) => void;
}

const NavContext = createContext<NavValue | null>(null);

function read(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}
function write(key: string, value: string | null) {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {
    /* storage unavailable: per-viewer convenience only */
  }
}

export function NavProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<NavState>(() => ({
    view: (read("wr.view") as View) || "overview",
    projectId: read("wr.project"),
    selection: null,
    inspectorOpen: read("wr.inspector") !== "closed",
    recovery: null,
    timelineWorktree: null,
  }));

  const go = useCallback<NavValue["go"]>((view, opts) => {
    write("wr.view", view === "recovery" ? "timeline" : view);
    setState((s) => ({
      ...s,
      view,
      recovery: view === "recovery" ? s.recovery : null,
      selection: opts?.selection !== undefined ? opts.selection : s.selection,
      timelineWorktree: opts?.timelineWorktree !== undefined ? opts.timelineWorktree : s.timelineWorktree,
    }));
  }, []);
  const select = useCallback((selection: Selection) => {
    setState((s) => ({ ...s, selection, inspectorOpen: selection ? true : s.inspectorOpen }));
  }, []);
  const setProject = useCallback((projectId: string | null) => {
    write("wr.project", projectId);
    setState((s) => ({ ...s, projectId, selection: null, recovery: null, timelineWorktree: null }));
  }, []);
  const setInspectorOpen = useCallback((open: boolean) => {
    write("wr.inspector", open ? "open" : "closed");
    setState((s) => ({ ...s, inspectorOpen: open }));
  }, []);
  const startRecovery = useCallback((recovery: RecoveryIntent | null) => {
    setState((s) => ({ ...s, view: "recovery", recovery }));
  }, []);

  const value = useMemo(
    () => ({ ...state, go, select, setProject, setInspectorOpen, startRecovery }),
    [state, go, select, setProject, setInspectorOpen, startRecovery],
  );
  return <NavContext.Provider value={value}>{children}</NavContext.Provider>;
}

export function useNav(): NavValue {
  const v = useContext(NavContext);
  if (!v) throw new Error("useNav outside NavProvider");
  return v;
}
