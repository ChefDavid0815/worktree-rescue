import clsx from "clsx";
import { Microscope, ScanSearch } from "lucide-react";
import { useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useI18n } from "../../lib/i18n";
import { api, CommandError, newTaskId } from "../../lib/ipc";
import { formatBytes, formatRelative, shortHash } from "../../lib/format";
import type { ForensicCandidate, ForensicReport, Project, RecoveryReport } from "../../lib/types";
import { Badge, Button, Dialog, ErrorBlock, Field, ProgressBar, TextInput, useToast } from "../../components/ui";

/**
 * Read-only Git forensics: reflog, stash entries and (on request) objects no
 * longer reachable from any ref. Every candidate keeps its provenance label;
 * none is presented as a Worktree Rescue snapshot.
 */
export function ForensicsPanel({ project }: { project: Project }) {
  const { t, td, locale } = useI18n();
  const [report, setReport] = useState<ForensicReport | null>(null);
  const [running, setRunning] = useState<null | { deep: boolean; taskId: string }>(null);
  const [error, setError] = useState<unknown>(null);
  const [open, setOpen] = useState<ForensicCandidate | null>(null);
  const [filter, setFilter] = useState<string>("all");

  const scan = async (deep: boolean) => {
    const taskId = newTaskId();
    setRunning({ deep, taskId });
    setError(null);
    try {
      setReport(await api.forensicScan(project.id, null, deep, taskId));
    } catch (e) {
      if (!(e instanceof CommandError && e.code === "cancelled")) setError(e);
    } finally {
      setRunning(null);
    }
  };

  const list = (report?.candidates ?? []).filter((c) => filter === "all" || c.provenance === filter);

  return (
    <section className="section">
      <div className="section-head">
        <div>
          <h2 className="section-title">{t("forensics.title")}</h2>
          <p className="section-note">{t("forensics.note")}</p>
        </div>
        <div className="page-actions">
          {running ? (
            <Button variant="ghost" onClick={() => api.cancelTask(running.taskId)}>
              {t("common.cancel")}
            </Button>
          ) : (
            <>
              <Button icon={<ScanSearch size={15} />} onClick={() => scan(false)}>
                {t("forensics.scan")}
              </Button>
              <Button icon={<Microscope size={15} />} onClick={() => scan(true)} title={t("forensics.deepHint")}>
                {t("forensics.deep")}
              </Button>
            </>
          )}
        </div>
      </div>
      {running && (
        <div className="forensic-run">
          <ProgressBar indeterminate />
          <p className="muted">{running.deep ? t("forensics.runningDeep") : t("forensics.running")}</p>
        </div>
      )}
      {error !== null && <ErrorBlock error={error} />}
      {report && (
        <>
          <p className="filter-note">
            {t("forensics.summary", {
              reflog: report.reflogEntries,
              stash: report.stashEntries,
              commits: report.unreachableCommits,
              blobs: report.unreachableBlobs,
            })}
          </p>
          <div className="chips">
            {["all", "gitReflog", "gitStash", "unreachableCommit", "unreachableBlob"].map((f) => (
              <button key={f} className={clsx("chip", filter === f && "is-on")} onClick={() => setFilter(f)} aria-pressed={filter === f}>
                {f === "all" ? t("common.all") : t(`provenance.${f}` as "provenance.gitReflog")}
              </button>
            ))}
          </div>
          {list.length === 0 ? (
            <p className="quiet-note">{t("forensics.none")}</p>
          ) : (
            <ul className="pick-list forensic-list">
              {list.slice(0, 200).map((c, i) => (
                <li key={`${c.provenance}-${c.oid}-${i}`}>
                  <button className="pick-row" onClick={() => setOpen(c)}>
                    <Badge tone="violet">{t(`provenance.${c.provenance}` as "provenance.gitReflog")}</Badge>
                    <span className="pick-main">
                      <span className="pick-title">{c.subject ?? (c.preview ? c.preview.split("\n")[0] : td("forensics.blob", "blob"))}</span>
                      <span className="pick-sub">
                        <span className="mono">{shortHash(c.oid, 10)}</span>
                        {c.reference && <span className="mono">{c.reference}</span>}
                        {c.authorTime && <span>{formatRelative(c.authorTime, locale)}</span>}
                        {c.size !== null && <span>{formatBytes(c.size)}</span>}
                      </span>
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
          {report.notes.length > 0 && <p className="note-line muted mono">{report.notes.join(" · ")}</p>}
        </>
      )}
      {open && <CandidateDialog project={project} candidate={open} onClose={() => setOpen(null)} />}
    </section>
  );
}

function CandidateDialog({ project, candidate, onClose }: { project: Project; candidate: ForensicCandidate; onClose: () => void }) {
  const { t } = useI18n();
  const qc = useQueryClient();
  const toast = useToast();
  const [files, setFiles] = useState<[string, string][] | null>(null);
  const [target, setTarget] = useState("");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<RecoveryReport | null>(null);
  const [error, setError] = useState<unknown>(null);

  useEffect(() => {
    if (candidate.objectType === "commit") {
      void api.forensicCommitFiles(project.id, candidate.oid).then(setFiles).catch(() => setFiles([]));
    }
    void api.suggestForensicTarget(project.id, candidate.oid).then(setTarget).catch(() => {});
  }, [project.id, candidate.oid, candidate.objectType]);

  const recover = async () => {
    setBusy(true);
    setError(null);
    try {
      const r = await api.recoverGitObject(project.id, candidate, target);
      setResult(r);
      await qc.invalidateQueries();
      if (r.status === "completed") toast({ tone: "cyan", title: t("report.toast.completed"), body: r.target });
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open
      onClose={onClose}
      title={candidate.subject ?? shortHash(candidate.oid, 12)}
      width={640}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {t("common.close")}
          </Button>
          {!result && (
            <Button variant="primary" onClick={recover} loading={busy} disabled={!target.trim()}>
              {t("forensics.recover")}
            </Button>
          )}
          {result && result.status !== "aborted" && (
            <Button variant="primary" onClick={() => api.revealPath(result.target)}>
              {t("report.open")}
            </Button>
          )}
        </>
      }
    >
      <div className="badge-row">
        <Badge tone="violet">{t(`provenance.${candidate.provenance}` as "provenance.gitReflog")}</Badge>
        <span className="mono">{candidate.oid}</span>
      </div>
      <p>{t(`provenance.${candidate.provenance}.limits` as "provenance.gitReflog.limits")}</p>
      {candidate.preview && <pre className="code-block mono selectable">{candidate.preview}</pre>}
      {files && (
        <div>
          <h3 className="block-title">{t("forensics.files", { count: files.length })}</h3>
          <ul className="commit-files">
            {files.slice(0, 200).map(([st, p]) => (
              <li key={p}>
                <span className="cat">{st[0]}</span>
                <span className="mono">{p}</span>
              </li>
            ))}
          </ul>
        </div>
      )}
      <Field label={t("recovery.target.label")} hint={t("forensics.targetHint")}>
        <TextInput value={target} onChange={(e) => setTarget(e.target.value)} mono />
      </Field>
      {error !== null && <ErrorBlock error={error} />}
      {result && (
        <p className={result.status === "completed" ? "tone-text-cyan" : "tone-text-amber"}>
          {t(`report.title.${result.status}` as "report.title.completed")} — {t("report.counts", { written: result.written, failed: result.failed, bytes: formatBytes(result.bytesWritten) })}
        </p>
      )}
    </Dialog>
  );
}
