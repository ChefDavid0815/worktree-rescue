import { X } from "lucide-react";
import { useI18n } from "../lib/i18n";
import { useEvents } from "../lib/queries";
import { describeEvent } from "../lib/describe";
import { formatDateTime } from "../lib/format";
import type { Project } from "../lib/types";
import { Badge, IconButton, PathText } from "../components/ui";
import { useNav } from "./nav";
import { WorktreeInspector } from "../features/worktrees/WorktreeInspector";
import { SnapshotInspector } from "../features/snapshots/SnapshotInspector";
import { RiskInspector } from "../features/risks/RiskInspector";

export function Inspector({ project }: { project: Project }) {
  const { t } = useI18n();
  const nav = useNav();
  const sel = nav.selection;
  const kind =
    sel?.type === "worktree"
      ? t("inspector.worktree")
      : sel?.type === "snapshot"
        ? t("inspector.snapshot")
        : sel?.type === "risk"
          ? t("inspector.risk")
          : sel?.type === "event"
            ? t("inspector.event")
            : t("inspector.title");
  const key = sel ? `${sel.type}:${"id" in sel ? sel.id : sel.path}` : "none";
  return (
    <aside className="inspector" aria-label={t("inspector.title")}>
      <div className="inspector-head">
        <span className="inspector-kind">{kind}</span>
        <IconButton label={t("topbar.hideInspector")} icon={<X size={16} />} onClick={() => nav.setInspectorOpen(false)} />
      </div>
      <div key={key} className="inspector-body view-enter-x">
          {sel?.type === "worktree" && <WorktreeInspector project={project} path={sel.path} />}
          {sel?.type === "snapshot" && <SnapshotInspector project={project} snapshotId={sel.id} />}
          {sel?.type === "risk" && <RiskInspector project={project} riskId={sel.id} />}
          {sel?.type === "event" && <EventInspector project={project} id={sel.id} />}
          {!sel && <p className="inspector-empty">{t("inspector.empty")}</p>}
      </div>
    </aside>
  );
}

function EventInspector({ project, id }: { project: Project; id: number }) {
  const { t, td, locale } = useI18n();
  const nav = useNav();
  const events = useEvents({ projectId: project.id, limit: 2000 });
  const e = events.data?.find((x) => x.id === id);
  if (!e) return <p className="inspector-empty">{t("inspector.notFound")}</p>;
  const look = describeEvent(e, td);
  return (
    <>
      <div>
        <Badge tone={look.tone} icon={look.icon}>
          {td(`eventKind.${e.kind}`, e.kind)}
        </Badge>
        <h2 className="inspector-title" style={{ marginTop: 10 }}>
          {look.title}
        </h2>
      </div>
      <dl className="facts">
        <dt>{t("facts.time")}</dt>
        <dd className="mono">{formatDateTime(e.at, locale)}</dd>
        {e.worktreePath && (
          <>
            <dt>{t("facts.worktree")}</dt>
            <dd>
              <PathText path={e.worktreePath} max={44} />
            </dd>
          </>
        )}
      </dl>
      {e.snapshotId && (
        <div className="action-row">
          <button className="btn btn-secondary btn-sm" onClick={() => nav.select({ type: "snapshot", id: e.snapshotId! })}>
            {t("inspector.openSnapshot")}
          </button>
        </div>
      )}
      <details className="tech">
        <summary>{t("common.technicalDetails")}</summary>
        <pre className="mono selectable">{JSON.stringify(e.data, null, 2)}</pre>
      </details>
    </>
  );
}
