import {
  AlertOctagon,
  ArrowRightLeft,
  Camera,
  CircleCheck,
  CircleDashed,
  FolderX,
  GitBranch,
  GitCommitHorizontal,
  Lock,
  LockOpen,
  PencilLine,
  Pause,
  Plus,
  ShieldAlert,
  ShieldCheck,
  Trash2,
  Undo2,
  XCircle,
} from "lucide-react";
import type { ReactNode } from "react";
import type { Tone } from "../components/ui";
import { baseName, shortHash } from "./format";
import type { TimelineEvent } from "./types";

type TD = (key: string, fallback?: string, params?: Record<string, string | number | null | undefined>) => string;

export interface EventLook {
  title: string;
  tone: Tone;
  icon: ReactNode;
}

const s = (v: unknown) => (v === null || v === undefined ? "" : String(v));

export function describeEvent(e: TimelineEvent, td: TD): EventLook {
  const d = e.data ?? {};
  const where = e.worktreePath ? baseName(e.worktreePath) : "";
  const p = { where, ...Object.fromEntries(Object.entries(d).map(([k, v]) => [k, typeof v === "object" ? "" : s(v)])) };
  const title = td(`event.${e.kind}`, e.kind, p);
  const sz = 14;
  switch (e.kind) {
    case "snapshotCreated":
      return { title, tone: "cyan", icon: <Camera size={sz} /> };
    case "snapshotPartial":
      return { title, tone: "amber", icon: <CircleDashed size={sz} /> };
    case "snapshotFailed":
    case "snapshotInterrupted":
      return { title, tone: "red", icon: <XCircle size={sz} /> };
    case "snapshotsPruned":
      return { title, tone: "neutral", icon: <Trash2 size={sz} /> };
    case "branchChanged":
      return {
        title: td("event.branchChanged", e.kind, { ...p, from: s(d.from) || td("common.detached"), to: s(d.to) || td("common.detached") }),
        tone: "neutral",
        icon: <GitBranch size={sz} />,
      };
    case "headMoved":
      return {
        title: td("event.headMoved", e.kind, { ...p, from: shortHash(s(d.from)), to: shortHash(s(d.to)) }),
        tone: "neutral",
        icon: <GitCommitHorizontal size={sz} />,
      };
    case "worktreeAdded":
    case "projectAdded":
      return { title, tone: "neutral", icon: <Plus size={sz} /> };
    case "worktreeUnreachable":
    case "worktreeRemovedFromGit":
      return {
        title: td(`event.${e.kind}`, e.kind, { ...p, diagnosis: td(`diagnosis.${s(d.diagnosis)}`, s(d.diagnosis)) }),
        tone: "red",
        icon: <FolderX size={sz} />,
      };
    case "worktreeReappeared":
      return { title, tone: "cyan", icon: <CircleCheck size={sz} /> };
    case "worktreeLocked":
      return { title, tone: "neutral", icon: <Lock size={sz} /> };
    case "worktreeUnlocked":
      return { title, tone: "neutral", icon: <LockOpen size={sz} /> };
    case "workingTreeChanged":
      return { title, tone: "neutral", icon: <PencilLine size={sz} /> };
    case "workingTreeClean":
      return { title, tone: "neutral", icon: <CircleCheck size={sz} /> };
    case "riskRaised":
      return {
        title: td("event.riskRaised", e.kind, { ...p, rule: td(`risk.${s(d.rule)}.title`, s(d.rule)) }),
        tone: d.severity === "critical" ? "red" : d.severity === "attention" ? "amber" : "neutral",
        icon: d.severity === "critical" ? <AlertOctagon size={sz} /> : <ShieldAlert size={sz} />,
      };
    case "riskResolved":
      return {
        title: td("event.riskResolved", e.kind, { ...p, rule: td(`risk.${s(d.rule)}.title`, s(d.rule)) }),
        tone: "cyan",
        icon: <ShieldCheck size={sz} />,
      };
    case "recoveryCompleted":
      return { title, tone: "cyan", icon: <Undo2 size={sz} /> };
    case "recoveryFailed":
      return { title, tone: "red", icon: <Undo2 size={sz} /> };
    case "monitoringChanged":
    case "autoProtectChanged":
      return {
        title: td(`event.${e.kind}.${d.enabled === true || d.monitoring === "active" ? "on" : "off"}`, e.kind, p),
        tone: "neutral",
        icon: <Pause size={sz} />,
      };
    case "integrityChecked":
      return {
        title: td(d.ok ? "event.integrityChecked.ok" : "event.integrityChecked.bad", e.kind, p),
        tone: d.ok ? "cyan" : "red",
        icon: d.ok ? <ShieldCheck size={sz} /> : <AlertOctagon size={sz} />,
      };
    default:
      return { title, tone: "neutral", icon: <ArrowRightLeft size={sz} /> };
  }
}
