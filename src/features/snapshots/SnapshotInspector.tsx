import clsx from "clsx";
import { FileDiff, LifeBuoy, Pin, PinOff, ShieldCheck, Trash2 } from "lucide-react";
import { useMemo, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useI18n } from "../../lib/i18n";
import { api } from "../../lib/ipc";
import { useSnapshotDetail } from "../../lib/queries";
import { formatBytes, formatDateTime, formatRelative } from "../../lib/format";
import { snapshotTone } from "../../lib/protection";
import type { Project, SnapshotFile } from "../../lib/types";
import { Badge, Button, Dialog, ErrorBlock, Hash, PathText, Skeleton, useToast } from "../../components/ui";
import { useNav } from "../../app/nav";
import { FileList } from "./FileList";
import { DiffDialog } from "./DiffDialog";

export function SnapshotInspector({ project, snapshotId }: { project: Project; snapshotId: string }) {
  const { t, td, locale } = useI18n();
  const nav = useNav();
  const qc = useQueryClient();
  const toast = useToast();
  const detail = useSnapshotDetail(snapshotId);
  const [verifying, setVerifying] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [diffFile, setDiffFile] = useState<SnapshotFile | null>(null);
  const [filter, setFilter] = useState<"all" | "captured" | "notCaptured">("all");

  const files = useMemo(() => {
    const all = detail.data?.files ?? [];
    if (filter === "captured") return all.filter((f) => f.wtState === "present" || f.wtState === "absent");
    if (filter === "notCaptured") return all.filter((f) => f.wtState === "skipped" || f.wtState === "failed");
    return all;
  }, [detail.data, filter]);

  if (detail.isLoading) return <Skeleton h={220} />;
  if (detail.error) return <ErrorBlock error={detail.error} onRetry={() => detail.refetch()} />;
  const d = detail.data!;
  const s = d.summary;
  const recoverable = s.status === "verified" || s.status === "partial";

  const verify = async () => {
    setVerifying(true);
    try {
      const r = await api.verifySnapshot(s.id, true);
      toast(
        r.ok
          ? { tone: "cyan", title: t("snapshot.verify.ok"), body: t("snapshot.verify.okBody", { count: r.blobsChecked }) }
          : { tone: "red", title: t("snapshot.verify.bad"), body: r.problems.map((p) => p.join(": ")).join("\n") },
      );
      await qc.invalidateQueries({ queryKey: ["snapshotDetail", s.id] });
    } catch (e) {
      toast({ tone: "red", title: t("toast.actionFailed"), body: (e as Error).message });
    } finally {
      setVerifying(false);
    }
  };

  const togglePin = async () => {
    await api.setSnapshotPinned(s.id, !s.pinned);
    await qc.invalidateQueries();
  };

  const remove = async () => {
    try {
      const r = await api.deleteSnapshots(project.id, [s.id]);
      toast({ tone: "neutral", title: t("snapshot.deleted"), body: t("storage.freed", { bytes: formatBytes(r.bytesFreed) }) });
      nav.select(null);
      await qc.invalidateQueries();
    } catch (e) {
      toast({ tone: "red", title: t("toast.actionFailed"), body: (e as Error).message });
    } finally {
      setConfirmDelete(false);
    }
  };

  return (
    <>
      <div>
        <div className="badge-row">
          <Badge tone={snapshotTone(s.status)} icon={s.status === "verified" ? <ShieldCheck size={12} /> : undefined}>
            {t(`snapshot.status.${s.status}` as "snapshot.status.verified")}
          </Badge>
          <Badge>{t(`snapshot.trigger.${s.trigger}` as "snapshot.trigger.manual")}</Badge>
          {s.pinned && <Badge tone="violet">{t("snapshot.pinned")}</Badge>}
        </div>
        <h2 className="inspector-title" style={{ marginTop: 10 }}>
          {s.note || formatDateTime(s.createdAt, locale)}
        </h2>
        <p className="insp-state">{t(`snapshot.statusExplain.${s.status}` as "snapshot.statusExplain.verified")}</p>
      </div>

      <div className="action-row">
        {recoverable && (
          <Button size="sm" variant="primary" icon={<LifeBuoy size={14} />} onClick={() => nav.startRecovery({ snapshotId: s.id })}>
            {t("snapshot.recover")}
          </Button>
        )}
        {recoverable && (
          <Button size="sm" icon={<ShieldCheck size={14} />} onClick={verify} loading={verifying}>
            {t("snapshot.verify.action")}
          </Button>
        )}
        <Button size="sm" variant="ghost" icon={s.pinned ? <PinOff size={14} /> : <Pin size={14} />} onClick={togglePin}>
          {s.pinned ? t("snapshot.unpin") : t("snapshot.pin")}
        </Button>
        {!s.pinned && (
          <Button size="sm" variant="ghost" icon={<Trash2 size={14} />} onClick={() => setConfirmDelete(true)}>
            {t("common.delete")}
          </Button>
        )}
      </div>

      {s.errorMessage && (
        <ErrorBlock error={Object.assign(new Error(s.errorMessage), { code: s.errorCode ?? "other" })} />
      )}

      <dl className="facts">
        <dt>{t("facts.created")}</dt>
        <dd className="mono">{formatDateTime(s.createdAt, locale)}</dd>
        <dt>{t("facts.worktree")}</dt>
        <dd>
          <PathText path={s.worktreePath} max={46} />
        </dd>
        <dt>{t("facts.branch")}</dt>
        <dd className="mono">{s.branch ?? (s.detached ? t("common.detached") : "—")}</dd>
        <dt>{t("facts.head")}</dt>
        <dd>{d.git?.unborn ? t("snapshot.unborn") : <Hash value={s.headOid} n={12} />}</dd>
        <dt>{t("facts.files")}</dt>
        <dd className="num">
          {t("snapshot.fileBreakdown", {
            total: s.entryCount,
            staged: s.stagedCount,
            unstaged: s.unstagedCount,
            untracked: s.untrackedCount,
            deleted: s.deletedCount,
          })}
        </dd>
        <dt>{t("facts.protected")}</dt>
        <dd className="num">
          {t("snapshot.protectedLine", { captured: s.capturedCount, bytes: formatBytes(s.protectedBytes), added: formatBytes(s.newBytes) })}
        </dd>
        {(s.skippedCount > 0 || s.failedCount > 0) && (
          <>
            <dt>{t("facts.notCaptured")}</dt>
            <dd className="tone-text-amber">{t("snapshot.notCapturedLine", { skipped: s.skippedCount, failed: s.failedCount })}</dd>
          </>
        )}
        {d.ignoredNotProtected > 0 && (
          <>
            <dt>{t("facts.gitIgnored")}</dt>
            <dd className="muted">{t("snapshot.ignoredLine", { count: d.ignoredNotProtected })}</dd>
          </>
        )}
        <dt>{t("facts.integrity")}</dt>
        <dd>
          {d.manifestOk ? (
            <span className="tone-text-cyan">
              {t("snapshot.manifestOk")}
              {s.verifiedAt ? ` — ${formatRelative(s.verifiedAt, locale)}` : ""}
            </span>
          ) : (
            <span className="tone-text-red">{d.manifestError ?? t("snapshot.manifestMissing")}</span>
          )}
        </dd>
        <dt>{t("facts.manifest")}</dt>
        <dd>
          <Hash value={s.manifestSha256} n={16} />
        </dd>
      </dl>

      <div className="insp-files">
        <div className="insp-files-head">
          <h3 className="block-title">{t("snapshot.files")}</h3>
          <div className="mini-tabs" role="tablist">
            {(["all", "captured", "notCaptured"] as const).map((f) => (
              <button key={f} role="tab" aria-selected={filter === f} className={clsx(filter === f && "is-active")} onClick={() => setFilter(f)}>
                {t(`snapshot.filter.${f}` as "snapshot.filter.all")}
              </button>
            ))}
          </div>
        </div>
        <FileList files={files} onOpen={(f) => setDiffFile(f)} compact />
        {files.length > 0 && (
          <p className="note-line muted">
            <FileDiff size={12} /> {t("snapshot.diffHint")}
          </p>
        )}
      </div>

      {diffFile && (
        <DiffDialog project={project} snapshot={s} file={diffFile} onClose={() => setDiffFile(null)} onRecover={(paths) => nav.startRecovery({ snapshotId: s.id, paths })} />
      )}

      <Dialog
        open={confirmDelete}
        onClose={() => setConfirmDelete(false)}
        title={t("snapshot.deleteTitle")}
        tone="red"
        footer={
          <>
            <Button variant="ghost" onClick={() => setConfirmDelete(false)}>
              {t("common.cancel")}
            </Button>
            <Button variant="danger" icon={<Trash2 size={14} />} onClick={remove}>
              {t("snapshot.deleteConfirm")}
            </Button>
          </>
        }
      >
        <p>{t("snapshot.deleteBody")}</p>
        <p className="muted">{td("snapshot.deleteShared", "")}</p>
      </Dialog>
    </>
  );
}
