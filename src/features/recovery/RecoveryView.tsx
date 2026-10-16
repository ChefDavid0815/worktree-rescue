import clsx from "clsx";
import { ArrowLeft, FolderOpen, LifeBuoy, ShieldCheck } from "lucide-react";
import { useI18n } from "../../lib/i18n";
import { api } from "../../lib/ipc";
import { useRecoveries, useSnapshots } from "../../lib/queries";
import { baseName, formatBytes, formatDateTime, formatRelative } from "../../lib/format";
import { snapshotTone } from "../../lib/protection";
import type { Project } from "../../lib/types";
import { Badge, Button, EmptyState, PathText, SkeletonRows, useToast } from "../../components/ui";
import { useNav } from "../../app/nav";
import { RecoveryWorkspace } from "./RecoveryWorkspace";
import { ForensicsPanel } from "./ForensicsPanel";

export function RecoveryView({ project }: { project: Project }) {
  const nav = useNav();
  const { t } = useI18n();
  if (nav.recovery) {
    return (
      <div className="page">
        <button className="back-link" onClick={() => nav.startRecovery(null)}>
          <ArrowLeft size={14} /> {t("recovery.back")}
        </button>
        <RecoveryWorkspace key={nav.recovery.snapshotId + (nav.recovery.paths?.join("|") ?? "")} project={project} intent={nav.recovery} />
      </div>
    );
  }
  return <RecoveryLanding project={project} />;
}

function RecoveryLanding({ project }: { project: Project }) {
  const { t, locale } = useI18n();
  const nav = useNav();
  const toast = useToast();
  const snaps = useSnapshots({ projectId: project.id, limit: 30 });
  const recoveries = useRecoveries(project.id);
  const recoverable = (snaps.data ?? []).filter((s) => s.status === "verified" || s.status === "partial");

  return (
    <div className="page">
      <header className="page-header">
        <div>
          <h1 className="page-title">{t("recovery.title")}</h1>
          <p className="page-sub">{t("recovery.sub")}</p>
        </div>
      </header>

      <div className="provenance-legend">
        <span>
          <Badge tone="cyan" icon={<ShieldCheck size={12} />}>{t("provenance.verifiedLocalSnapshot")}</Badge>
          {t("provenance.verifiedLocalSnapshot.desc")}
        </span>
        <span>
          <Badge tone="violet">{t("provenance.gitReflog")}</Badge>
          {t("provenance.git.desc")}
        </span>
        <span>
          <Badge>{t("provenance.currentWorkingTree")}</Badge>
          {t("provenance.currentWorkingTree.desc")}
        </span>
      </div>

      <section className="section">
        <div className="section-head">
          <h2 className="section-title">{t("recovery.fromSnapshot")}</h2>
          <button className="btn btn-quiet btn-sm" onClick={() => nav.go("timeline")}>
            {t("recovery.browseAll")}
          </button>
        </div>
        {snaps.isLoading && <SkeletonRows rows={4} />}
        {!snaps.isLoading && recoverable.length === 0 && (
          <EmptyState compact title={t("recovery.noSnapshots.title")} body={t("recovery.noSnapshots.body")} />
        )}
        <ul className="pick-list">
          {recoverable.slice(0, 12).map((s) => (
            <li key={s.id}>
              <div className="pick-row">
                <span className={clsx("v-dot", `tone-${snapshotTone(s.status)}`)} />
                <span className="pick-main">
                  <span className="pick-title">{s.note || formatDateTime(s.createdAt, locale)}</span>
                  <span className="pick-sub">
                    <span className="mono">{s.branch ?? t("common.detached")}</span>
                    <span>{baseName(s.worktreePath)}</span>
                    <span>{t("snapshot.filesCount", { count: s.entryCount })}</span>
                    <span>{formatRelative(s.createdAt, locale)}</span>
                  </span>
                </span>
                <Button size="sm" icon={<LifeBuoy size={14} />} onClick={() => nav.startRecovery({ snapshotId: s.id })}>
                  {t("recovery.recoverThis")}
                </Button>
              </div>
            </li>
          ))}
        </ul>
      </section>

      <ForensicsPanel project={project} />

      <section className="section">
        <div className="section-head">
          <h2 className="section-title">{t("recovery.history")}</h2>
        </div>
        {(recoveries.data ?? []).length === 0 ? (
          <p className="quiet-note">{t("recovery.historyNone")}</p>
        ) : (
          <ul className="pick-list">
            {recoveries.data!.map((r) => (
              <li key={r.id}>
                <div className="pick-row">
                  <Badge tone={r.status === "completed" ? "cyan" : r.status === "completedWithIssues" ? "amber" : "red"}>
                    {t(`report.status.${r.status}` as "report.status.completed")}
                  </Badge>
                  <span className="pick-main">
                    <span className="pick-title">
                      {t(`recovery.mode.${r.mode}` as "recovery.mode.separateDirectory")} —{" "}
                      {t(`provenance.${r.report?.provenance ?? "verifiedLocalSnapshot"}` as "provenance.verifiedLocalSnapshot")}
                    </span>
                    <span className="pick-sub">
                      <PathText path={r.target} max={70} />
                      <span>{formatRelative(r.startedAt, locale)}</span>
                      {r.report && (
                        <span>
                          {t("report.counts", { written: r.report.written, failed: r.report.failed, bytes: formatBytes(r.report.bytesWritten) })}
                        </span>
                      )}
                    </span>
                  </span>
                  {r.mode === "separateDirectory" && (
                    <Button
                      size="sm"
                      variant="ghost"
                      icon={<FolderOpen size={14} />}
                      onClick={() => api.revealPath(r.target).catch((e) => toast({ tone: "red", title: t("toast.actionFailed"), body: e.message }))}
                    >
                      {t("common.reveal")}
                    </Button>
                  )}
                </div>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
