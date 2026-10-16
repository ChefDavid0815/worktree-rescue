import clsx from "clsx";
import { useVirtualizer } from "@tanstack/react-virtual";
import { Link2 } from "lucide-react";
import { useRef } from "react";
import { useI18n } from "../../lib/i18n";
import { formatBytes } from "../../lib/format";
import type { SnapshotFile } from "../../lib/types";
import { Checkbox } from "../../components/ui";

const LETTER: Record<string, string> = {
  modified: "M",
  added: "A",
  deleted: "D",
  renamed: "R",
  copied: "C",
  typeChanged: "T",
  untracked: "U",
  conflicted: "!",
  ignoredIncluded: "I",
  unchanged: "·",
};

export function categoryLetter(c: string) {
  return LETTER[c] ?? "?";
}

export function FileList({
  files,
  onOpen,
  selectable,
  selected,
  onToggle,
  compact,
  height = 420,
}: {
  files: SnapshotFile[];
  onOpen?: (f: SnapshotFile) => void;
  selectable?: (f: SnapshotFile) => boolean;
  selected?: Set<string>;
  onToggle?: (path: string, on: boolean) => void;
  compact?: boolean;
  height?: number;
}) {
  const { t, td } = useI18n();
  const ref = useRef<HTMLDivElement>(null);
  const virt = useVirtualizer({
    count: files.length,
    getScrollElement: () => ref.current,
    estimateSize: () => (compact ? 34 : 38),
    overscan: 16,
    initialRect: { width: 800, height },
  });
  if (files.length === 0) return <p className="quiet-note">{t("files.none")}</p>;
  return (
    <div className={clsx("file-list", compact && "is-compact")} ref={ref} style={{ maxHeight: height }} role="list">
      <div style={{ height: virt.getTotalSize(), position: "relative" }}>
        {virt.getVirtualItems().map((vi) => {
          const f = files[vi.index];
          const slash = f.path.lastIndexOf("/");
          const dir = slash >= 0 ? f.path.slice(0, slash + 1) : "";
          const name = slash >= 0 ? f.path.slice(slash + 1) : f.path;
          const notCaptured = f.wtState === "skipped" || f.wtState === "failed";
          const canSelect = selectable ? selectable(f) : false;
          return (
            <div
              key={f.path}
              className={clsx("file-row", notCaptured && "is-dim", f.wtState === "absent" && "is-deleted")}
              style={{ transform: `translateY(${vi.start}px)`, height: vi.size }}
              role="listitem"
            >
              {onToggle && (
                <Checkbox
                  checked={!!selected?.has(f.path)}
                  disabled={!canSelect}
                  onChange={(v) => onToggle(f.path, v)}
                  label={f.path}
                />
              )}
              <span className={clsx("cat", `cat-${f.category}`)} title={td(`category.${f.category}`, f.category)}>
                {categoryLetter(f.category)}
              </span>
              <button className="file-name" onClick={() => onOpen?.(f)} disabled={!onOpen} title={f.origPath ? `${f.origPath} → ${f.path}` : f.path}>
                <span className="fn-dir mono">{dir}</span>
                <span className="fn-base mono">{name}</span>
                {f.kind === "symlink" && <Link2 size={11} className="fn-link" aria-label={t("files.symlink")} />}
              </button>
              <span className="file-meta">
                {notCaptured ? (
                  <span className={clsx("skip", f.wtState === "failed" && "is-failed")}>
                    {td(`skip.${f.skipReason}`, f.skipReason ?? "")}
                  </span>
                ) : f.wtState === "absent" ? (
                  <span className="muted">{t("files.deleted")}</span>
                ) : (
                  <span className="mono muted">{formatBytes(f.wtSize)}</span>
                )}
                {f.indexBlob && f.indexBlob !== f.wtBlob && <span className="staged-flag" title={t("files.stagedDiffers")}>S</span>}
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
