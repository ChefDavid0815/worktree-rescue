import { Camera, FolderOpen, History, LifeBuoy, Terminal } from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { useI18n } from "../../lib/i18n";
import { useSettings, useWorktrees } from "../../lib/queries";
import { api } from "../../lib/ipc";
import { formatDateTime, formatRelative, samePath } from "../../lib/format";
import { diagnosisTone, protectionState } from "../../lib/protection";
import type { Project } from "../../lib/types";
import { Badge, Button, Hash, PathText, ProtectionRing, useToast } from "../../components/ui";
import { useNav } from "../../app/nav";
import { CaptureDialog } from "../snapshots/CaptureDialog";

export function WorktreeInspector({ project, path }: { project: Project; path: string }) {
  const { t, td, locale } = useI18n();
  const nav = useNav();
  const toast = useToast();
  const settings = useSettings();
  const wts = useWorktrees(project.id);
  const w = wts.data?.find((x) => samePath(x.path, path));
  const [capture, setCapture] = useState(false);
  const guidance = useQuery({
    queryKey: ["guidance", project.id, path, w?.diagnosis],
    queryFn: () => api.worktreeGuidance(project.id, path),
    enabled: !!w && w.diagnosis !== "healthy",
  });
  if (!w) return <p className="inspector-empty">{t("inspector.notFound")}</p>;
  const state = protectionState(w, project, settings.data?.staleProtectionMinutes ?? 120);
  const healthy = w.diagnosis === "healthy" && !w.goneFromGit;
  const c = w.counts;

  return (
    <>
      <div className="insp-hero">
        <ProtectionRing state={state} size={40} />
        <div>
          <h2 className="inspector-title mono">{w.branch ?? t("common.detached")}</h2>
          <p className="insp-state">{t(`ring.${state}.long` as "ring.protected.long")}</p>
        </div>
      </div>

      <div className="action-row">
        {healthy && (
          <Button size="sm" variant="primary" icon={<Camera size={14} />} onClick={() => setCapture(true)}>
            {t("topbar.capture")}
          </Button>
        )}
        <Button
          size="sm"
          icon={<History size={14} />}
          onClick={() => nav.go("timeline", { timelineWorktree: w.path })}
        >
          {t("worktree.viewTimeline")}
        </Button>
        {w.lastSnapshotId && (
          <Button size="sm" icon={<LifeBuoy size={14} />} onClick={() => nav.startRecovery({ snapshotId: w.lastSnapshotId! })}>
            {t("worktree.recoverLatest")}
          </Button>
        )}
        {healthy && (
          <Button
            size="sm"
            variant="ghost"
            icon={<FolderOpen size={14} />}
            onClick={() => api.revealPath(w.path).catch((e) => toast({ tone: "red", title: t("toast.actionFailed"), body: e.message }))}
          >
            {t("common.reveal")}
          </Button>
        )}
      </div>

      <dl className="facts">
        <dt>{t("facts.path")}</dt>
        <dd>
          <PathText path={w.path} max={48} />
        </dd>
        <dt>{t("facts.role")}</dt>
        <dd>{w.isMain ? t("worktree.main") : t("worktree.linked")}</dd>
        <dt>{t("facts.head")}</dt>
        <dd>
          <Hash value={w.headOid} n={12} />
          {w.detached && <Badge className="ml">{t("common.detached")}</Badge>}
        </dd>
        <dt>{t("facts.diagnosis")}</dt>
        <dd>
          <Badge tone={diagnosisTone(w.diagnosis)}>{td(`diagnosis.${w.diagnosis}`, w.diagnosis)}</Badge>
        </dd>
        {w.locked && (
          <>
            <dt>{t("worktree.locked")}</dt>
            <dd>{w.lockReason || t("worktree.lockedNoReason")}</dd>
          </>
        )}
        {c && (
          <>
            <dt>{t("facts.changes")}</dt>
            <dd className="num">
              {t("worktree.countsLine", {
                staged: c.staged,
                unstaged: c.unstaged,
                untracked: c.untracked,
                deleted: c.deleted,
              })}
            </dd>
          </>
        )}
        <dt>{t("facts.lastChange")}</dt>
        <dd>{w.lastDirtyAt ? formatRelative(w.lastDirtyAt, locale) : "—"}</dd>
        <dt>{t("facts.lastProtected")}</dt>
        <dd>{w.lastProtectedAt ? formatDateTime(w.lastProtectedAt, locale) : t("worktree.neverCaptured")}</dd>
        <dt>{t("facts.lastSeen")}</dt>
        <dd>{w.lastSeenAt ? formatRelative(w.lastSeenAt, locale) : "—"}</dd>
        <dt>{t("facts.lastChecked")}</dt>
        <dd>{formatRelative(w.lastCheckedAt, locale)}</dd>
      </dl>

      {w.evidence.length > 0 && (
        <div>
          <h3 className="block-title">{t("worktree.evidence")}</h3>
          <ul className="evidence-list">
            {w.evidence.map((e) => (
              <li key={e}>{td(`evidence.${e}`, e)}</li>
            ))}
          </ul>
          {w.possibleNewPath && (
            <p className="note-line">
              {t("worktree.possibleNewPath")} <PathText path={w.possibleNewPath} max={52} />
            </p>
          )}
        </div>
      )}

      {!healthy && (
        <div className="guidance">
          <h3 className="block-title">{t("worktree.guidance.title")}</h3>
          <p className="note-line">{td(`guidance.${w.diagnosis}`, t("guidance.generic"))}</p>
          {guidance.data && guidance.data.filter((g) => g.startsWith("git ")).length > 0 && (
            <>
              <p className="note-line muted">{t("worktree.guidance.preview")}</p>
              <pre className="code-block mono selectable">
                <Terminal size={12} aria-hidden />
                {guidance.data.filter((g) => g.startsWith("git ")).join("\n")}
              </pre>
            </>
          )}
        </div>
      )}
      <CaptureDialog open={capture} onClose={() => setCapture(false)} project={project} initialWorktree={w.path} />
    </>
  );
}
