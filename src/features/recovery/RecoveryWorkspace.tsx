import clsx from "clsx";
import { AlertTriangle, Check, FolderOpen, FolderSearch, LifeBuoy, ShieldCheck, X } from "lucide-react";
import { motion } from "motion/react";
import { useEffect, useMemo, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useI18n } from "../../lib/i18n";
import { api, isTauri, newTaskId } from "../../lib/ipc";
import { clearProgress, useSnapshotDetail } from "../../lib/queries";
import { baseName, formatBytes, formatDateTime, shortHash } from "../../lib/format";
import { snapshotTone } from "../../lib/protection";
import type { Project, RecoveryReport, RestorePreview, SnapshotFile, Variant } from "../../lib/types";
import type { RecoveryIntent } from "../../app/nav";
import { useNav } from "../../app/nav";
import {
  Badge,
  Button,
  Checkbox,
  ErrorBlock,
  Field,
  Hash,
  PathText,
  Segmented,
  Skeleton,
  TextInput,
  Toggle,
  useToast,
} from "../../components/ui";
import { FileList } from "../snapshots/FileList";

const PHASES = [
  "sourceVerification",
  "targetSelection",
  "conflictDetection",
  "changePreview",
  "recoveryExecution",
  "postRecoveryValidation",
] as const;

type Mode = "separateDirectory" | "restoreSelectedFiles";
const MAX_IN_PLACE = 200;

export function RecoveryWorkspace({ project, intent }: { project: Project; intent: RecoveryIntent }) {
  const { t, locale } = useI18n();
  const detail = useSnapshotDetail(intent.snapshotId);
  const [mode, setMode] = useState<Mode>(intent.mode ?? "separateDirectory");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [report, setReport] = useState<RecoveryReport | null>(null);

  const recoverableFile = (f: SnapshotFile) =>
    mode === "separateDirectory" ? f.wtState === "present" || !!f.indexBlob : f.wtState === "present" && f.kind === "file";

  useEffect(() => {
    if (!detail.data) return;
    const files = detail.data.files;
    if (intent.paths?.length) setSelected(new Set(intent.paths));
    else setSelected(new Set(files.filter((f) => f.wtState === "present" || f.indexBlob).map((f) => f.path)));
  }, [detail.data, intent.paths]);

  if (detail.isLoading) return <Skeleton h={300} />;
  if (detail.error) return <ErrorBlock error={detail.error} />;
  const d = detail.data!;
  const s = d.summary;
  const selectable = d.files.filter(recoverableFile);
  const selCount = selectable.filter((f) => selected.has(f.path)).length;
  const allOn = selCount === selectable.length && selectable.length > 0;

  return (
    <div className="recovery">
      <header className="rw-head">
        <div>
          <h1 className="page-title">{t("recovery.workspace.title")}</h1>
          <p className="page-sub">
            {t("recovery.workspace.sub", {
              when: formatDateTime(s.createdAt, locale),
              branch: s.branch ?? t("common.detached"),
              worktree: baseName(s.worktreePath),
            })}
          </p>
        </div>
        <div className="badge-row">
          <Badge tone="cyan" icon={<ShieldCheck size={12} />}>
            {t("provenance.verifiedLocalSnapshot")}
          </Badge>
          <Badge tone={snapshotTone(s.status)}>{t(`snapshot.status.${s.status}` as "snapshot.status.verified")}</Badge>
          <Hash value={s.id} n={8} />
        </div>
      </header>

      <PhaseRail report={report} />

      {report ? (
        <ReportPanel report={report} onReset={() => setReport(null)} />
      ) : (
        <>
          <div className="mode-choice" role="radiogroup" aria-label={t("recovery.mode.label")}>
            <button
              role="radio"
              aria-checked={mode === "separateDirectory"}
              className={clsx("mode-card", mode === "separateDirectory" && "is-active")}
              onClick={() => setMode("separateDirectory")}
            >
              <span className="mode-title">
                {t("recovery.mode.separateDirectory")}
                <Badge tone="cyan">{t("common.recommended")}</Badge>
              </span>
              <span className="mode-desc">{t("recovery.mode.separateDirectory.desc")}</span>
            </button>
            <button
              role="radio"
              aria-checked={mode === "restoreSelectedFiles"}
              className={clsx("mode-card", mode === "restoreSelectedFiles" && "is-active")}
              onClick={() => setMode("restoreSelectedFiles")}
            >
              <span className="mode-title">{t("recovery.mode.restoreSelectedFiles")}</span>
              <span className="mode-desc">{t("recovery.mode.restoreSelectedFiles.desc")}</span>
            </button>
          </div>

          <div className="rw-grid">
            <section className="rw-files">
              <div className="section-head">
                <h2 className="section-title">{t("recovery.files.title")}</h2>
                <label className="select-all">
                  <Checkbox
                    checked={allOn}
                    indeterminate={selCount > 0 && !allOn}
                    onChange={(v) => setSelected(v ? new Set(selectable.map((f) => f.path)) : new Set())}
                    label={t("recovery.files.selectAll")}
                  />
                  <span>{t("recovery.files.selected", { count: selCount, total: selectable.length })}</span>
                </label>
              </div>
              <FileList
                files={d.files}
                selectable={recoverableFile}
                selected={selected}
                onToggle={(p, on) =>
                  setSelected((prev) => {
                    const n = new Set(prev);
                    if (on) n.add(p);
                    else n.delete(p);
                    return n;
                  })
                }
                height={480}
              />
              {d.files.some((f) => !recoverableFile(f)) && <p className="note-line muted">{t(`recovery.files.unavailable.${mode}` as "recovery.files.unavailable.separateDirectory")}</p>}
            </section>
            <section className="rw-side">
              {mode === "separateDirectory" ? (
                <SeparateDirectoryForm
                  snapshotId={s.id}
                  selection={[...selected].filter((p) => selectable.some((f) => f.path === p))}
                  allSelected={allOn}
                  onDone={setReport}
                />
              ) : (
                <InPlaceForm
                  project={project}
                  snapshotId={s.id}
                  worktreePath={s.worktreePath}
                  selection={[...selected].filter((p) => selectable.some((f) => f.path === p))}
                  onDone={setReport}
                />
              )}
            </section>
          </div>
        </>
      )}
    </div>
  );
}

function PhaseRail({ report }: { report: RecoveryReport | null }) {
  const { t } = useI18n();
  return (
    <ol className="phase-rail">
      {PHASES.map((p, i) => {
        const rec = report?.phases.find((x) => x.phase === p);
        const state = rec ? (rec.ok ? "ok" : "bad") : report ? "skipped" : "pending";
        return (
          <li key={p} className={clsx("pr-step", `is-${state}`)} title={rec?.detail}>
            <span className="pr-mark">{state === "ok" ? <Check size={12} /> : state === "bad" ? <X size={12} /> : i + 1}</span>
            <span className="pr-label">{t(`phase.${p}` as "phase.sourceVerification")}</span>
          </li>
        );
      })}
    </ol>
  );
}

function SeparateDirectoryForm({
  snapshotId,
  selection,
  allSelected,
  onDone,
}: {
  snapshotId: string;
  selection: string[];
  allSelected: boolean;
  onDone: (r: RecoveryReport) => void;
}) {
  const { t } = useI18n();
  const qc = useQueryClient();
  const toast = useToast();
  const [target, setTarget] = useState("");
  const [includeStaged, setIncludeStaged] = useState(true);
  const [includeHead, setIncludeHead] = useState(false);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<unknown>(null);

  useEffect(() => {
    void api.suggestRecoveryTarget(snapshotId).then(setTarget).catch(() => {});
  }, [snapshotId]);

  const browse = async () => {
    if (!isTauri()) return;
    const dir = await openDialog({ directory: true, multiple: false, title: t("recovery.target.browse") });
    if (typeof dir === "string") setTarget(`${dir.replace(/[\\/]+$/, "")}\\${baseName(target) || "recovered"}`);
  };

  const run = async () => {
    setRunning(true);
    setError(null);
    try {
      const r = await api.recoverToDirectory({
        snapshotId,
        target,
        selection: allSelected ? null : selection,
        includeStaged,
        includeHeadFiles: includeHead,
      });
      onDone(r);
      await qc.invalidateQueries();
      if (r.status === "completed") toast({ tone: "cyan", title: t("report.toast.completed"), body: r.target });
    } catch (e) {
      setError(e);
    } finally {
      setRunning(false);
    }
  };

  return (
    <div className="rw-form">
      <h2 className="section-title">{t("recovery.target.title")}</h2>
      <Field label={t("recovery.target.label")} hint={t("recovery.target.hint")}>
        <div className="input-with-btn">
          <TextInput value={target} onChange={(e) => setTarget(e.target.value)} mono aria-label={t("recovery.target.label")} />
          <Button icon={<FolderSearch size={14} />} onClick={browse} disabled={!isTauri()}>
            {t("recovery.target.browse")}
          </Button>
        </div>
      </Field>
      <Toggle checked={includeStaged} onChange={setIncludeStaged} label={t("recovery.opt.staged")} description={t("recovery.opt.staged.desc")} />
      <Toggle checked={includeHead} onChange={setIncludeHead} label={t("recovery.opt.head")} description={t("recovery.opt.head.desc")} />
      <div className="rw-summary">
        <p>{t("recovery.separate.summary", { count: selection.length })}</p>
        <p className="muted">{t("recovery.separate.safety")}</p>
      </div>
      {error !== null && <ErrorBlock error={error} />}
      <Button variant="primary" size="lg" icon={<LifeBuoy size={16} />} onClick={run} loading={running} disabled={!target.trim() || selection.length === 0}>
        {t("recovery.separate.run", { count: selection.length })}
      </Button>
    </div>
  );
}

function InPlaceForm({
  project,
  snapshotId,
  worktreePath,
  selection,
  onDone,
}: {
  project: Project;
  snapshotId: string;
  worktreePath: string;
  selection: string[];
  onDone: (r: RecoveryReport) => void;
}) {
  const { t, td } = useI18n();
  const qc = useQueryClient();
  const [variant, setVariant] = useState<Variant>("workingTree");
  const [preview, setPreview] = useState<RestorePreview | null>(null);
  const [ack, setAck] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const selKey = useMemo(() => selection.join("\n") + variant, [selection, variant]);
  useEffect(() => {
    setPreview(null);
    setAck(false);
  }, [selKey]);

  const tooMany = selection.length > MAX_IN_PLACE;

  const buildPreview = async () => {
    setBusy(true);
    setError(null);
    try {
      setPreview(await api.previewRestore(snapshotId, selection, variant));
      setAck(false);
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  };

  const execute = async () => {
    if (!preview) return;
    setBusy(true);
    setError(null);
    const taskId = newTaskId();
    try {
      const r = await api.executeRestore(project.id, preview.id, taskId);
      onDone(r);
      await qc.invalidateQueries();
    } catch (e) {
      setError(e);
      setPreview(null);
    } finally {
      clearProgress(taskId);
      setBusy(false);
    }
  };

  const actionable = preview ? preview.overwriteCount + preview.createCount : 0;

  return (
    <div className="rw-form">
      <h2 className="section-title">{t("recovery.inplace.title")}</h2>
      <p className="muted">{t("recovery.inplace.target")}</p>
      <PathText path={worktreePath} max={60} />
      <Field label={t("recovery.inplace.variant")}>
        <Segmented
          label={t("recovery.inplace.variant")}
          value={variant}
          onChange={setVariant}
          options={[
            { value: "workingTree", label: t("variant.workingTree") },
            { value: "staged", label: t("variant.staged") },
          ]}
        />
      </Field>
      {tooMany && <p className="warn-line">{t("recovery.inplace.tooMany", { max: MAX_IN_PLACE })}</p>}
      {!preview && (
        <Button onClick={buildPreview} loading={busy} disabled={selection.length === 0 || tooMany}>
          {t("recovery.inplace.preview", { count: selection.length })}
        </Button>
      )}
      {error !== null && <ErrorBlock error={error} />}
      {preview && (
        <motion.div className="preview" initial={{ y: 6 }} animate={{ y: 0 }}>
          <p className="preview-summary">
            {t("recovery.inplace.previewSummary", {
              overwrite: preview.overwriteCount,
              create: preview.createCount,
              blocked: preview.blockedCount,
            })}
          </p>
          <ul className="preview-list">
            {preview.items.map((it) => (
              <li key={it.path} className={clsx("pv-row", `pv-${it.action}`)}>
                <Badge tone={it.action === "overwrite" ? "amber" : it.action === "create" ? "cyan" : it.action === "blocked" ? "red" : "neutral"}>
                  {t(`preview.action.${it.action}` as "preview.action.create")}
                </Badge>
                <span className="pv-path mono" title={it.path}>
                  {it.path}
                </span>
                <span className="pv-hash mono muted">
                  {it.action === "blocked"
                    ? td(`blocked.${it.blockedReason?.split(":")[0]}`, it.blockedReason ?? "")
                    : `${shortHash(it.currentSha256, 7)} → ${shortHash(it.snapshotSha256, 7)}`}
                </span>
                <span className="pv-size mono muted">
                  {it.currentSize !== null ? formatBytes(it.currentSize) : "—"} → {formatBytes(it.snapshotSize)}
                </span>
              </li>
            ))}
          </ul>
          {actionable > 0 ? (
            <>
              <div className="confirm-box">
                <AlertTriangle size={16} />
                <div>
                  <p>{t("recovery.inplace.warning", { count: preview.overwriteCount })}</p>
                  <label className="ack">
                    <Checkbox checked={ack} onChange={setAck} label={t("recovery.inplace.ack")} />
                    <span>{t("recovery.inplace.ack")}</span>
                  </label>
                </div>
              </div>
              <div className="action-row">
                <Button variant="ghost" onClick={() => setPreview(null)}>
                  {t("recovery.inplace.rebuild")}
                </Button>
                <Button variant="primary" onClick={execute} loading={busy} disabled={!ack}>
                  {t("recovery.inplace.execute", { count: actionable })}
                </Button>
              </div>
            </>
          ) : (
            <p className="muted">{t("recovery.inplace.nothing")}</p>
          )}
        </motion.div>
      )}
    </div>
  );
}

function ReportPanel({ report, onReset }: { report: RecoveryReport; onReset: () => void }) {
  const { t, td } = useI18n();
  const nav = useNav();
  const toast = useToast();
  const ok = report.status === "completed";
  const failures = report.items.filter((i) => i.outcome === "failed");
  return (
    <motion.section className={clsx("report", `report-${report.status}`)} initial={{ y: 8 }} animate={{ y: 0 }} transition={{ duration: 0.3 }}>
      <div className="report-head">
        <div className={clsx("result-mark", ok ? "tone-cyan" : report.status === "aborted" ? "tone-amber" : "tone-red")}>
          {ok ? <ShieldCheck size={24} /> : <AlertTriangle size={22} />}
        </div>
        <div>
          <h2 className="report-title">{t(`report.title.${report.status}` as "report.title.completed")}</h2>
          <p className="muted">{t(`report.explain.${report.status}` as "report.explain.completed")}</p>
        </div>
      </div>
      <dl className="report-stats">
        <div>
          <dt>{t("report.written")}</dt>
          <dd className="num">{report.written}</dd>
        </div>
        <div>
          <dt>{t("report.skipped")}</dt>
          <dd className="num">{report.skipped}</dd>
        </div>
        <div>
          <dt>{t("report.failed")}</dt>
          <dd className={clsx("num", report.failed > 0 && "tone-text-red")}>{report.failed}</dd>
        </div>
        <div>
          <dt>{t("report.bytes")}</dt>
          <dd className="num">{formatBytes(report.bytesWritten)}</dd>
        </div>
      </dl>
      <ul className="phase-log">
        {report.phases.map((p) => (
          <li key={p.phase} className={p.ok ? "ok" : "bad"}>
            {p.ok ? <Check size={13} /> : <X size={13} />}
            <span className="pl-phase">{t(`phase.${p.phase}` as "phase.sourceVerification")}</span>
            <span className="pl-detail mono">{p.detail}</span>
          </li>
        ))}
      </ul>
      {report.mode === "separateDirectory" && (
        <p className="note-line">
          {t("report.location")} <PathText path={report.target} max={80} />
        </p>
      )}
      {report.protectionSnapshotId && (
        <p className="note-line">
          {t("report.protection")}{" "}
          <button className="link-btn" onClick={() => nav.select({ type: "snapshot", id: report.protectionSnapshotId! })}>
            {report.protectionSnapshotId.slice(0, 8)}
          </button>
        </p>
      )}
      {report.mode === "restoreSelectedFiles" && <p className="note-line muted">{t("report.indexUntouched")}</p>}
      {failures.length > 0 && (
        <div>
          <h3 className="block-title">{t("report.failures")}</h3>
          <ul className="failure-list">
            {failures.map((f) => (
              <li key={f.path + (f.variant ?? "")}>
                <span className="mono">{f.path}</span>
                <span className="muted">{td(`blocked.${f.detail}`, f.detail ?? "")}</span>
              </li>
            ))}
          </ul>
        </div>
      )}
      <div className="action-row">
        {report.mode === "separateDirectory" && report.status !== "aborted" && (
          <Button
            variant="primary"
            icon={<FolderOpen size={15} />}
            onClick={() => api.revealPath(report.target).catch((e) => toast({ tone: "red", title: t("toast.actionFailed"), body: e.message }))}
          >
            {t("report.open")}
          </Button>
        )}
        <Button onClick={onReset}>{t("report.again")}</Button>
      </div>
    </motion.section>
  );
}
