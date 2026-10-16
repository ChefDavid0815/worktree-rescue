import clsx from "clsx";
import { lazy, Suspense, useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Columns2, LifeBuoy, Rows2, X } from "lucide-react";
import { useI18n } from "../../lib/i18n";
import { api } from "../../lib/ipc";
import { useFileHistory } from "../../lib/queries";
import { formatBytes, formatDateTime, formatRelative } from "../../lib/format";
import { snapshotTone } from "../../lib/protection";
import type { BlobContent, Project, SnapshotFile, SnapshotSummary } from "../../lib/types";
import { Badge, Button, ErrorBlock, Hash, IconButton, Segmented, Skeleton } from "../../components/ui";
import { categoryLetter } from "./FileList";

const MonacoDiff = lazy(() => import("./MonacoDiff"));

type Mode = "head" | "current" | "staged";
const EMPTY: BlobContent = { sha256: null, size: 0, binary: false, truncated: false, text: "", source: "none" };

function useContent(key: unknown[], fn: (() => Promise<BlobContent>) | null) {
  return useQuery({ queryKey: ["content", ...key], queryFn: fn ?? (async () => EMPTY), staleTime: 60_000, retry: 0 });
}

export function DiffDialog({
  project,
  snapshot,
  file,
  onClose,
  onRecover,
}: {
  project: Project;
  snapshot: SnapshotSummary;
  file: SnapshotFile;
  onClose: () => void;
  onRecover: (paths: string[]) => void;
}) {
  const { t, td, locale } = useI18n();
  const history = useFileHistory(project.id, file.path);
  const [versionId, setVersionId] = useState(snapshot.id);
  const [mode, setMode] = useState<Mode>("head");
  const [sideBySide, setSideBySide] = useState(true);
  const theme = (document.documentElement.dataset.theme as "dark" | "light") ?? "dark";

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const version = useMemo(() => {
    const fromHistory = history.data?.find((v) => v.snapshotId === versionId);
    return fromHistory ? { file: fromHistory.file, worktreePath: fromHistory.worktreePath } : { file, worktreePath: snapshot.worktreePath };
  }, [history.data, versionId, file, snapshot.worktreePath]);
  const f = version.file;

  const snapContent = useContent(["snap", f.wtBlob], f.wtBlob ? () => api.blobContent(project.id, f.wtBlob!) : null);
  const headContent = useContent(["head", f.headOid], f.headOid ? () => api.gitBlobContent(project.id, f.headOid!) : null);
  const stagedContent = useContent(["staged", f.indexBlob], f.indexBlob ? () => api.blobContent(project.id, f.indexBlob!) : null);
  const currentContent = useQuery({
    queryKey: ["content", "current", version.worktreePath, f.path],
    queryFn: () => api.currentFileContent(project.id, version.worktreePath, f.path),
    enabled: mode === "current",
    retry: 0,
  });

  const sides: { original: typeof snapContent; modified: typeof snapContent; labels: [string, string] } =
    mode === "head"
      ? { original: headContent, modified: snapContent, labels: [t("diff.side.head"), t("diff.side.snapshot")] }
      : mode === "staged"
        ? { original: stagedContent, modified: snapContent, labels: [t("diff.side.staged"), t("diff.side.workingTree")] }
        : { original: snapContent, modified: currentContent as typeof snapContent, labels: [t("diff.side.snapshot"), t("diff.side.current")] };

  const loading = sides.original.isLoading || sides.modified.isLoading;
  const error = sides.original.error || sides.modified.error;
  const binary = sides.original.data?.binary || sides.modified.data?.binary || f.binary;
  const captured = f.wtState === "present" && !!f.wtBlob;

  return (
    <div className="diff-overlay" role="dialog" aria-modal="true" aria-label={t("diff.title")}>
      <div className="diff-window">
        <header className="diff-head">
          <span className={clsx("cat", `cat-${f.category}`)}>{categoryLetter(f.category)}</span>
          <div className="diff-title">
            <span className="mono selectable">{f.path}</span>
            {f.origPath && <span className="muted mono">← {f.origPath}</span>}
          </div>
          <Segmented
            label={t("diff.compare")}
            size="sm"
            value={mode}
            onChange={setMode}
            options={[
              { value: "head", label: t("diff.mode.head") },
              { value: "current", label: t("diff.mode.current") },
              ...(f.indexBlob ? [{ value: "staged" as Mode, label: t("diff.mode.staged") }] : []),
            ]}
          />
          <IconButton
            label={sideBySide ? t("diff.inline") : t("diff.sideBySide")}
            icon={sideBySide ? <Rows2 size={16} /> : <Columns2 size={16} />}
            onClick={() => setSideBySide((s) => !s)}
          />
          {captured && (
            <Button size="sm" icon={<LifeBuoy size={14} />} onClick={() => { onRecover([f.path]); onClose(); }}>
              {t("diff.recoverFile")}
            </Button>
          )}
          <IconButton label={t("common.close")} icon={<X size={17} />} onClick={onClose} />
        </header>
        <div className="diff-body">
          <aside className="diff-versions">
            <h3 className="block-title">{t("diff.versions")}</h3>
            {history.isLoading && <Skeleton h={120} />}
            <ul>
              {(history.data ?? []).map((v) => (
                <li key={v.snapshotId}>
                  <button className={clsx("version", v.snapshotId === versionId && "is-active")} onClick={() => setVersionId(v.snapshotId)}>
                    <span className={clsx("v-dot", `tone-${snapshotTone(v.status)}`)} />
                    <span className="v-text">
                      <span className="v-when">{formatRelative(v.createdAt, locale)}</span>
                      <span className="v-sub mono">
                        {v.branch ?? "—"} · {v.file.wtState === "absent" ? t("files.deleted") : formatBytes(v.file.wtSize)}
                      </span>
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          </aside>
          <section className="diff-main">
            <div className="diff-sides">
              <span>
                <Badge>{sides.labels[0]}</Badge>
                <Hash value={sides.original.data?.sha256 ?? (mode === "head" ? f.headOid : null)} n={10} />
              </span>
              <span>
                <Badge tone="cyan">{sides.labels[1]}</Badge>
                <Hash value={sides.modified.data?.sha256} n={10} />
              </span>
            </div>
            {error ? (
              <div className="diff-pad">
                <ErrorBlock error={error} />
                {mode === "current" && <p className="note-line muted">{t("diff.currentUnavailable")}</p>}
              </div>
            ) : loading ? (
              <div className="diff-pad">
                <Skeleton h={300} />
              </div>
            ) : binary ? (
              <div className="diff-pad binary-meta">
                <h3>{t("diff.binary")}</h3>
                <dl className="facts">
                  <dt>{sides.labels[0]}</dt>
                  <dd className="mono">{formatBytes(sides.original.data?.size)} — {sides.original.data?.sha256 ?? "—"}</dd>
                  <dt>{sides.labels[1]}</dt>
                  <dd className="mono">{formatBytes(sides.modified.data?.size)} — {sides.modified.data?.sha256 ?? "—"}</dd>
                  {f.lfsPointer && (
                    <>
                      <dt>LFS</dt>
                      <dd>{t("diff.lfsPointer")}</dd>
                    </>
                  )}
                </dl>
              </div>
            ) : f.wtState === "skipped" || f.wtState === "failed" ? (
              <div className="diff-pad">
                <p>{t("diff.notCaptured", { reason: td(`skip.${f.skipReason}`, f.skipReason ?? "") })}</p>
              </div>
            ) : (
              <Suspense fallback={<div className="diff-pad"><Skeleton h={300} /></div>}>
                <div className="diff-editor">
                  <MonacoDiff
                    original={sides.original.data?.text ?? ""}
                    modified={sides.modified.data?.text ?? ""}
                    path={f.path}
                    theme={theme}
                    sideBySide={sideBySide}
                  />
                </div>
              </Suspense>
            )}
            {(sides.original.data?.truncated || sides.modified.data?.truncated) && (
              <p className="diff-foot muted">{t("diff.truncated")}</p>
            )}
            <p className="diff-foot muted">
              {t("diff.provenance", {
                when: formatDateTime(history.data?.find((v) => v.snapshotId === versionId)?.createdAt ?? snapshot.createdAt, locale),
              })}
            </p>
          </section>
        </div>
      </div>
    </div>
  );
}
