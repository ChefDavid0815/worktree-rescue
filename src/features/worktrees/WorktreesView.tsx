import clsx from "clsx";
import { Lock, RefreshCw } from "lucide-react";
import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useI18n } from "../../lib/i18n";
import { useSettings, useWorktrees } from "../../lib/queries";
import { api } from "../../lib/ipc";
import { formatRelative, samePath } from "../../lib/format";
import { diagnosisTone, protectionState } from "../../lib/protection";
import type { Project } from "../../lib/types";
import { Badge, Button, EmptyState, ErrorBlock, Hash, PathText, ProtectionRing, SkeletonRows, useToast } from "../../components/ui";
import { useNav } from "../../app/nav";

export function WorktreesView({ project }: { project: Project }) {
  const { t, td, locale } = useI18n();
  const nav = useNav();
  const qc = useQueryClient();
  const toast = useToast();
  const settings = useSettings();
  const wts = useWorktrees(project.id);
  const [busy, setBusy] = useState(false);
  const stale = settings.data?.staleProtectionMinutes ?? 120;

  const refresh = async () => {
    setBusy(true);
    try {
      const r = await api.refreshProject(project.id);
      await qc.invalidateQueries();
      toast({ tone: "cyan", title: t("worktrees.refreshed", { count: r.worktrees.length }) });
    } catch (e) {
      toast({ tone: "red", title: t("toast.refreshFailed"), body: (e as Error).message });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="page page-wide">
      <header className="page-header">
        <div>
          <h1 className="page-title">{t("worktrees.title")}</h1>
          <p className="page-sub">{t("worktrees.sub")}</p>
        </div>
        <div className="page-actions">
          <Button icon={<RefreshCw size={15} className={busy ? "spin" : undefined} />} onClick={refresh} disabled={busy}>
            {t("worktrees.refresh")}
          </Button>
        </div>
      </header>
      {wts.error && <ErrorBlock error={wts.error} onRetry={() => wts.refetch()} />}
      {wts.isLoading && <SkeletonRows rows={4} />}
      {wts.data && wts.data.length === 0 && <EmptyState title={t("worktrees.empty")} />}
      {wts.data && wts.data.length > 0 && (
        <div className="table" role="table" aria-label={t("worktrees.title")}>
          <div className="table-head" role="row">
            <span role="columnheader" className="col-state" />
            <span role="columnheader">{t("worktrees.col.branch")}</span>
            <span role="columnheader">{t("worktrees.col.path")}</span>
            <span role="columnheader">{t("worktrees.col.head")}</span>
            <span role="columnheader">{t("worktrees.col.changes")}</span>
            <span role="columnheader">{t("worktrees.col.diagnosis")}</span>
            <span role="columnheader" className="col-right">{t("worktrees.col.protected")}</span>
          </div>
          {wts.data.map((w) => {
            const state = protectionState(w, project, stale);
            const selected = nav.selection?.type === "worktree" && samePath(nav.selection.path, w.path);
            const c = w.counts;
            return (
              <button
                key={w.id}
                role="row"
                className={clsx("table-row", selected && "is-selected", w.goneFromGit && "is-gone")}
                onClick={() => nav.select({ type: "worktree", path: w.path })}
              >
                <span role="cell" className="col-state">
                  <ProtectionRing state={state} size={22} title={t(`ring.${state}` as "ring.protected")} />
                </span>
                <span role="cell" className="cell-branch">
                  <span className="mono">{w.branch ?? t("common.detached")}</span>
                  {w.isMain && <Badge>{t("worktree.main")}</Badge>}
                  {w.locked && (
                    <Badge icon={<Lock size={11} />} title={w.lockReason ?? undefined}>
                      {t("worktree.locked")}
                    </Badge>
                  )}
                </span>
                <span role="cell" className="cell-path">
                  <PathText path={w.path} max={58} />
                </span>
                <span role="cell">
                  <Hash value={w.headOid} copy={false} />
                </span>
                <span role="cell" className="cell-counts num">
                  {c && w.diagnosis === "healthy" ? (
                    w.dirty ? (
                      <>
                        <span title={t("counts.staged")}>{c.staged}<i>S</i></span>
                        <span title={t("counts.unstaged")}>{c.unstaged}<i>M</i></span>
                        <span title={t("counts.untracked")}>{c.untracked}<i>?</i></span>
                        {c.deleted > 0 && <span title={t("counts.deleted")}>{c.deleted}<i>D</i></span>}
                      </>
                    ) : (
                      <span className="muted">{t("worktree.clean")}</span>
                    )
                  ) : (
                    <span className="muted">—</span>
                  )}
                </span>
                <span role="cell">
                  <Badge tone={diagnosisTone(w.diagnosis)}>{td(`diagnosis.${w.diagnosis}`, w.diagnosis)}</Badge>
                </span>
                <span role="cell" className="col-right muted">
                  {w.lastProtectedAt ? formatRelative(w.lastProtectedAt, locale) : t("worktree.neverCaptured")}
                </span>
              </button>
            );
          })}
        </div>
      )}
      <p className="footnote">{t("worktrees.footnote")}</p>
    </div>
  );
}
