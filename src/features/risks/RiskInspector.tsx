import { Camera, Check, GitFork, HardDrive, LifeBuoy } from "lucide-react";
import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useI18n, type Params } from "../../lib/i18n";
import { api } from "../../lib/ipc";
import { useRisks } from "../../lib/queries";
import { formatBytes, formatDateTime, formatRelative } from "../../lib/format";
import { severityTone } from "../../lib/protection";
import type { Project, Risk } from "../../lib/types";
import { Badge, Button, PathText } from "../../components/ui";
import { useNav } from "../../app/nav";
import { CaptureDialog } from "../snapshots/CaptureDialog";

/** Flatten evidence into translation parameters with friendly formatting. */
function params(r: Risk, td: (k: string, f?: string) => string, locale: "en" | "zh-CN"): Params {
  const e = r.evidence as Record<string, unknown>;
  const out: Params = {};
  for (const [k, v] of Object.entries(e)) {
    if (v === null || v === undefined) continue;
    if (typeof v === "number" && /At$/.test(k)) out[k] = formatRelative(v, locale);
    else if (typeof v === "number" && /Bytes$/.test(k)) out[k] = formatBytes(v);
    else if (typeof v === "number" && k === "fraction") out[k] = `${Math.round(v * 100)}%`;
    else if (typeof v !== "object") out[k] = String(v);
  }
  if (typeof e.diagnosis === "string") out.diagnosis = td(`diagnosis.${e.diagnosis}`, e.diagnosis);
  if (e.reasons && typeof e.reasons === "object") {
    out.reasons = Object.entries(e.reasons as Record<string, number>)
      .map(([k, n]) => `${td(`skip.${k}`, k)} (${n})`)
      .join(", ");
  }
  return out;
}

export function RiskInspector({ project, riskId }: { project: Project; riskId: string }) {
  const { t, td, locale } = useI18n();
  const nav = useNav();
  const qc = useQueryClient();
  const risks = useRisks(project.id, true);
  const [capture, setCapture] = useState(false);
  const r = risks.data?.find((x) => x.id === riskId);
  if (!r) return <p className="inspector-empty">{t("inspector.notFound")}</p>;
  const p = params(r, td, locale);
  const ev = r.evidence as Record<string, unknown>;
  const snapshotId = (ev.lastSnapshotId ?? ev.snapshotId) as string | undefined;

  const ack = async () => {
    await api.acknowledgeRisk(project.id, r.id);
    await qc.invalidateQueries({ queryKey: ["risks", project.id] });
  };

  return (
    <>
      <div>
        <div className="badge-row">
          <Badge tone={severityTone(r.severity)}>{t(`severity.${r.severity}` as "severity.info")}</Badge>
          <Badge title={t("risks.confidenceHint")}>{t(`confidence.${r.confidence}` as "confidence.high")}</Badge>
          {r.resolvedAt && <Badge tone="cyan">{t("risks.resolvedBadge")}</Badge>}
        </div>
        <h2 className="inspector-title" style={{ marginTop: 10 }}>
          {td(`risk.${r.rule}.title`, r.rule)}
        </h2>
      </div>
      <div className="explain">
        <h3 className="block-title">{t("risks.what")}</h3>
        <p>{td(`risk.${r.rule}.explain`, "", p)}</p>
      </div>
      <div className="explain">
        <h3 className="block-title">{t("risks.next")}</h3>
        <p>{td(`risk.${r.rule}.action`, "", p)}</p>
      </div>
      <div className="action-row">
        {snapshotId && (
          <Button size="sm" variant="primary" icon={<LifeBuoy size={14} />} onClick={() => nav.startRecovery({ snapshotId })}>
            {t("risks.recoverSnapshot")}
          </Button>
        )}
        {r.rule === "staleProtection" && (
          <Button size="sm" variant="primary" icon={<Camera size={14} />} onClick={() => setCapture(true)}>
            {t("topbar.capture")}
          </Button>
        )}
        {r.worktreePath && (
          <Button size="sm" icon={<GitFork size={14} />} onClick={() => nav.select({ type: "worktree", path: r.worktreePath! })}>
            {t("risks.openWorktree")}
          </Button>
        )}
        {(r.rule === "storageLow" || r.rule === "budgetNearlyFull") && (
          <Button size="sm" icon={<HardDrive size={14} />} onClick={() => nav.go("settings")}>
            {t("risks.openStorage")}
          </Button>
        )}
        {!r.acknowledged && !r.resolvedAt && (
          <Button size="sm" variant="ghost" icon={<Check size={14} />} onClick={ack}>
            {t("risks.acknowledge")}
          </Button>
        )}
      </div>
      <dl className="facts">
        <dt>{t("facts.detected")}</dt>
        <dd className="mono">{formatDateTime(r.detectedAt, locale)}</dd>
        <dt>{t("facts.lastObserved")}</dt>
        <dd>{formatRelative(r.lastSeenAt, locale)}</dd>
        {r.worktreePath && (
          <>
            <dt>{t("facts.worktree")}</dt>
            <dd>
              <PathText path={r.worktreePath} max={44} />
            </dd>
          </>
        )}
        <dt>{t("facts.rule")}</dt>
        <dd className="mono">{r.rule}</dd>
      </dl>
      <p className="note-line muted">{t("risks.noAttribution")}</p>
      <details className="tech">
        <summary>{t("common.technicalDetails")}</summary>
        <pre className="mono selectable">{JSON.stringify(r.evidence, null, 2)}</pre>
      </details>
      {r.worktreePath && (
        <CaptureDialog open={capture} onClose={() => setCapture(false)} project={project} initialWorktree={r.worktreePath} />
      )}
    </>
  );
}
