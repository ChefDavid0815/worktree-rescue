import type { RingState, Tone } from "../components/ui";
import type { Project, Severity, SnapshotStatus, Worktree, WorktreeDiagnosis } from "./types";

/**
 * Honest protection state of a worktree, derived only from observed facts:
 * reachability, whether it has uncommitted work, and when that work was last
 * captured (or confirmed unchanged) by a verified snapshot.
 */
export function protectionState(w: Worktree, project: Project | undefined, staleMinutes: number, now = Date.now()): RingState {
  if (w.goneFromGit || w.diagnosis !== "healthy") {
    return w.diagnosis === "volumeUnavailable" || w.diagnosis === "unknown" || w.diagnosis === "inaccessible"
      ? "unknown"
      : "missing";
  }
  if (project?.monitoring === "paused") return "paused";
  if (w.dirty === false) return "clean";
  if (w.dirty == null) return "unknown";
  const protectedAt = w.lastProtectedAt;
  if (!protectedAt) return "unprotected";
  const changedAfter = w.lastDirtyAt != null && w.lastDirtyAt > protectedAt;
  if (changedAfter) {
    return now - protectedAt > staleMinutes * 60_000 ? "unprotected" : "stale";
  }
  return "protected";
}

export function ringTone(state: RingState): Tone {
  switch (state) {
    case "protected":
      return "cyan";
    case "stale":
    case "unprotected":
      return "amber";
    case "missing":
      return "red";
    default:
      return "neutral";
  }
}

export function severityTone(s: Severity): Tone {
  return s === "critical" ? "red" : s === "attention" ? "amber" : "neutral";
}

export function snapshotTone(s: SnapshotStatus): Tone {
  switch (s) {
    case "verified":
      return "cyan";
    case "partial":
      return "amber";
    case "failed":
    case "interrupted":
      return "red";
    default:
      return "neutral";
  }
}

export function diagnosisTone(d: WorktreeDiagnosis): Tone {
  switch (d) {
    case "healthy":
      return "cyan";
    case "missing":
    case "prunableCandidate":
      return "red";
    case "possiblyMoved":
    case "volumeUnavailable":
    case "inaccessible":
      return "amber";
    default:
      return "neutral";
  }
}

export function changedCount(w: Worktree): number {
  const c = w.counts;
  if (!c) return 0;
  return c.staged + c.unstaged + c.untracked + c.conflicted;
}
