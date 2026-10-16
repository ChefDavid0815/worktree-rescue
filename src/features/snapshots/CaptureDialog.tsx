import clsx from "clsx";
import { Check, CircleDashed, ShieldCheck } from "lucide-react";
import { motion } from "motion/react";
import { useEffect, useMemo, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useI18n } from "../../lib/i18n";
import { api, CommandError, newTaskId } from "../../lib/ipc";
import { clearProgress, useProgress, useSettings, useWorktrees } from "../../lib/queries";
import { formatBytes, pathTail, samePath } from "../../lib/format";
import { changedCount, protectionState } from "../../lib/protection";
import type { CaptureOutcome, Project } from "../../lib/types";
import { Button, Dialog, ErrorBlock, ProgressBar, ProtectionRing } from "../../components/ui";
import { useNav } from "../../app/nav";

const PHASES = ["scanning", "capturing", "verifying", "committing"] as const;

export function CaptureDialog({
  open,
  onClose,
  project,
  initialWorktree,
}: {
  open: boolean;
  onClose: () => void;
  project: Project;
  initialWorktree?: string;
}) {
  const { t, tn } = useI18n();
  const nav = useNav();
  const qc = useQueryClient();
  const settings = useSettings();
  const wts = useWorktrees(project.id);
  const candidates = useMemo(() => (wts.data ?? []).filter((w) => w.diagnosis === "healthy" && !w.goneFromGit), [wts.data]);
  const [target, setTarget] = useState<string | null>(null);
  const [note, setNote] = useState("");
  const [taskId, setTaskId] = useState<string | null>(null);
  const [result, setResult] = useState<CaptureOutcome | null>(null);
  const [error, setError] = useState<unknown>(null);
  const progress = useProgress(taskId);

  useEffect(() => {
    if (!open) return;
    setResult(null);
    setError(null);
    setNote("");
    setTaskId(null);
    const preferred =
      (initialWorktree && candidates.find((w) => samePath(w.path, initialWorktree))) ||
      candidates.find((w) => w.dirty) ||
      candidates[0];
    setTarget(preferred?.path ?? null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const running = !!taskId && !result && !error;

  const start = async () => {
    if (!target) return;
    const id = newTaskId();
    setTaskId(id);
    setError(null);
    try {
      const out = await api.createSnapshot(project.id, target, note.trim() || null, id);
      setResult(out);
      await qc.invalidateQueries();
    } catch (e) {
      setError(e);
    } finally {
      clearProgress(id);
    }
  };

  const cancel = async () => {
    if (taskId) await api.cancelTask(taskId);
  };

  const phaseIndex = progress ? Math.max(0, PHASES.indexOf(progress.phase === "readingIndex" ? "scanning" : (progress.phase as (typeof PHASES)[number]))) : 0;
  const snap = result?.snapshot;
  const cancelled = error instanceof CommandError && error.code === "cancelled";

  return (
    <Dialog
      open={open}
      onClose={running ? () => {} : onClose}
      title={t("capture.title")}
      width={560}
      footer={
        result || error ? (
          <>
            {snap && (
              <Button
                onClick={() => {
                  nav.select({ type: "snapshot", id: snap.id });
                  onClose();
                }}
              >
                {t("capture.view")}
              </Button>
            )}
            <Button variant="primary" onClick={onClose}>
              {t("common.done")}
            </Button>
          </>
        ) : running ? (
          <Button variant="ghost" onClick={cancel}>
            {t("common.cancel")}
          </Button>
        ) : (
          <>
            <Button variant="ghost" onClick={onClose}>
              {t("common.cancel")}
            </Button>
            <Button variant="primary" onClick={start} disabled={!target}>
              {t("capture.start")}
            </Button>
          </>
        )
      }
    >
      {!running && !result && !error && (
        <>
          <p>{t("capture.intro")}</p>
          <div className="choice-list" role="radiogroup" aria-label={t("capture.worktree")}>
            {candidates.map((w) => {
              const state = protectionState(w, project, settings.data?.staleProtectionMinutes ?? 120);
              return (
                <button
                  key={w.id}
                  role="radio"
                  aria-checked={target === w.path}
                  className={clsx("choice", target === w.path && "is-active")}
                  onClick={() => setTarget(w.path)}
                >
                  <ProtectionRing state={state} size={22} />
                  <span className="choice-text">
                    <span className="mono">{w.branch ?? t("common.detached")}</span>
                    <span className="choice-sub">{w.isMain ? t("worktree.main") : pathTail(w.path, 2)}</span>
                  </span>
                  <span className="choice-meta">{w.dirty ? tn("worktree.changes", changedCount(w)) : t("worktree.clean")}</span>
                </button>
              );
            })}
            {candidates.length === 0 && <p className="quiet-note">{t("capture.noneReachable")}</p>}
          </div>
          <label className="field">
            <span className="field-label">{t("capture.note")}</span>
            <textarea className="input" rows={2} value={note} onChange={(e) => setNote(e.target.value)} placeholder={t("capture.notePlaceholder")} maxLength={500} />
          </label>
        </>
      )}

      {running && (
        <div className="capture-run" aria-live="polite">
          <ol className="phase-track">
            {PHASES.map((p, i) => (
              <li key={p} className={clsx(i < phaseIndex && "is-done", i === phaseIndex && "is-active")}>
                <span className="phase-dot">{i < phaseIndex ? <Check size={11} /> : null}</span>
                <span>{t(`capture.phase.${p}` as "capture.phase.scanning")}</span>
              </li>
            ))}
          </ol>
          <ProgressBar
            value={progress && progress.total > 0 ? progress.done / progress.total : undefined}
            indeterminate={!progress || progress.total === 0}
          />
          <p className="capture-current mono">
            {progress?.total ? `${progress.done}/${progress.total}  ` : ""}
            {progress?.current ?? ""}
          </p>
        </div>
      )}

      {result && (
        <motion.div className="capture-result" initial={{ y: 6 }} animate={{ y: 0 }}>
          {snap ? (
            <>
              <div className={clsx("result-mark", snap.status === "verified" ? "tone-cyan" : "tone-amber")}>
                {snap.status === "verified" ? <ShieldCheck size={22} /> : <CircleDashed size={22} />}
              </div>
              <div>
                <h3 className="result-title">
                  {snap.status === "verified" ? t("capture.result.verified") : t("capture.result.partial")}
                </h3>
                <p className="result-line">
                  {t("capture.result.line", {
                    captured: snap.capturedCount,
                    skipped: snap.skippedCount,
                    failed: snap.failedCount,
                    bytes: formatBytes(snap.newBytes),
                  })}
                </p>
                {snap.skippedCount > 0 && <p className="result-hint">{t("capture.result.skippedHint")}</p>}
              </div>
            </>
          ) : (
            <p>{t("capture.result.unchanged")}</p>
          )}
        </motion.div>
      )}
      {error !== null && (cancelled ? <p>{t("capture.cancelled")}</p> : <ErrorBlock error={error} />)}
    </Dialog>
  );
}
