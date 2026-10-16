import clsx from "clsx";
import { useVirtualizer } from "@tanstack/react-virtual";
import { Pin, Search, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useI18n } from "../../lib/i18n";
import { useEvents, useSnapshots, useWorktrees } from "../../lib/queries";
import { describeEvent } from "../../lib/describe";
import { baseName, formatBytes, formatDay, formatTime, samePath } from "../../lib/format";
import { snapshotTone } from "../../lib/protection";
import { useNow } from "../../lib/useNow";
import type { Project, SnapshotSummary, TimelineEvent } from "../../lib/types";
import { EmptyState, ErrorBlock, Segmented, SkeletonRows, TextInput } from "../../components/ui";
import { useNav } from "../../app/nav";

type Range = "24h" | "7d" | "30d" | "all";
type Kind = "snapshots" | "git" | "worktree" | "risks" | "recovery";
const KIND_OF: Record<string, Kind> = {
  branchChanged: "git",
  headMoved: "git",
  workingTreeChanged: "git",
  workingTreeClean: "git",
  worktreeAdded: "worktree",
  worktreeUnreachable: "worktree",
  worktreeReappeared: "worktree",
  worktreeRemovedFromGit: "worktree",
  worktreeLocked: "worktree",
  worktreeUnlocked: "worktree",
  projectAdded: "worktree",
  riskRaised: "risks",
  riskResolved: "risks",
  integrityChecked: "risks",
  recoveryCompleted: "recovery",
  recoveryFailed: "recovery",
};
const SNAPSHOT_EVENTS = new Set(["snapshotCreated", "snapshotPartial", "snapshotFailed"]);

type Item =
  | { type: "day"; key: string; at: number }
  | { type: "group"; key: string; path: string }
  | { type: "snapshot"; key: string; at: number; s: SnapshotSummary }
  | { type: "event"; key: string; at: number; e: TimelineEvent };

export function TimelineView({ project }: { project: Project }) {
  const { t, locale } = useI18n();
  const nav = useNav();
  const now = useNow();
  const [range, setRange] = useState<Range>("7d");
  const [worktree, setWorktree] = useState<string>(nav.timelineWorktree ?? "all");
  const [kinds, setKinds] = useState<Set<Kind>>(new Set(["snapshots", "git", "worktree", "risks", "recovery"]));
  const [search, setSearch] = useState("");
  const [query, setQuery] = useState("");
  const [grouped, setGrouped] = useState(false);
  useEffect(() => {
    const id = setTimeout(() => setQuery(search.trim()), 250);
    return () => clearTimeout(id);
  }, [search]);
  useEffect(() => {
    if (nav.timelineWorktree) setWorktree(nav.timelineWorktree);
  }, [nav.timelineWorktree]);

  const since = range === "all" ? null : now - { "24h": 1, "7d": 7, "30d": 30 }[range] * 86_400_000;
  const wts = useWorktrees(project.id);
  const wtPath = worktree === "all" ? null : worktree;
  const snaps = useSnapshots({ projectId: project.id, since, worktreePath: wtPath, pathContains: query || null, limit: 5000 });
  const events = useEvents({ projectId: project.id, since, worktreePath: wtPath, limit: 5000 });

  const items = useMemo<Item[]>(() => {
    const merged: Item[] = [];
    if (kinds.has("snapshots")) {
      for (const s of snaps.data ?? []) merged.push({ type: "snapshot", key: `s-${s.id}`, at: s.createdAt, s });
    }
    if (!query) {
      for (const e of events.data ?? []) {
        if (SNAPSHOT_EVENTS.has(e.kind)) continue;
        const k = KIND_OF[e.kind] ?? "worktree";
        if (!kinds.has(k)) continue;
        merged.push({ type: "event", key: `e-${e.id}`, at: e.at, e });
      }
    }
    merged.sort((a, b) => ("at" in b ? b.at : 0) - ("at" in a ? a.at : 0));
    const out: Item[] = [];
    if (grouped) {
      const byWt = new Map<string, Item[]>();
      for (const it of merged) {
        const p = it.type === "snapshot" ? it.s.worktreePath : it.type === "event" ? (it.e.worktreePath ?? "") : "";
        const key = [...byWt.keys()].find((k) => samePath(k, p)) ?? p;
        byWt.set(key, [...(byWt.get(key) ?? []), it]);
      }
      for (const [p, list] of byWt) {
        out.push({ type: "group", key: `g-${p}`, path: p });
        out.push(...list);
      }
      return out;
    }
    let lastDay = "";
    for (const it of merged) {
      const at = "at" in it ? it.at : 0;
      const day = new Date(at).toDateString();
      if (day !== lastDay) {
        out.push({ type: "day", key: `d-${day}`, at });
        lastDay = day;
      }
      out.push(it);
    }
    return out;
  }, [snaps.data, events.data, kinds, query, grouped]);

  const scrollRef = useRef<HTMLDivElement>(null);
  const virt = useVirtualizer({
    count: items.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: (i) => (items[i].type === "day" || items[i].type === "group" ? 40 : 54),
    overscan: 12,
    initialRect: { width: 1000, height: 800 },
  });

  const toggleKind = (k: Kind) =>
    setKinds((s) => {
      const n = new Set(s);
      if (n.has(k)) n.delete(k);
      else n.add(k);
      return n;
    });

  const loading = snaps.isLoading || events.isLoading;
  const error = snaps.error || events.error;
  const selectedSnapshot = nav.selection?.type === "snapshot" ? nav.selection.id : null;
  const selectedEvent = nav.selection?.type === "event" ? nav.selection.id : null;

  return (
    <div className="timeline-page">
      <header className="page-header timeline-header">
        <div>
          <h1 className="page-title">{t("timeline.title")}</h1>
          <p className="page-sub">{t("timeline.sub")}</p>
        </div>
      </header>
      <div className="filters">
        <Segmented
          label={t("timeline.range")}
          size="sm"
          value={range}
          onChange={setRange}
          options={[
            { value: "24h", label: t("range.24h") },
            { value: "7d", label: t("range.7d") },
            { value: "30d", label: t("range.30d") },
            { value: "all", label: t("range.all") },
          ]}
        />
        <select className="input select" value={worktree} onChange={(e) => setWorktree(e.target.value)} aria-label={t("timeline.worktree")}>
          <option value="all">{t("timeline.allWorktrees")}</option>
          {(wts.data ?? []).map((w) => (
            <option key={w.id} value={w.path}>
              {(w.branch ?? t("common.detached")) + "  —  " + (w.isMain ? t("worktree.main") : baseName(w.path))}
            </option>
          ))}
        </select>
        <div className="search">
          <Search size={14} aria-hidden />
          <TextInput
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder={t("timeline.searchFile")}
            aria-label={t("timeline.searchFile")}
            mono
          />
          {search && (
            <button className="search-clear" onClick={() => setSearch("")} aria-label={t("common.clear")}>
              <X size={13} />
            </button>
          )}
        </div>
        <div className="chips" role="group" aria-label={t("timeline.kinds")}>
          {(["snapshots", "git", "worktree", "risks", "recovery"] as Kind[]).map((k) => (
            <button key={k} className={clsx("chip", kinds.has(k) && "is-on")} aria-pressed={kinds.has(k)} onClick={() => toggleKind(k)}>
              {t(`timeline.kind.${k}` as "timeline.kind.snapshots")}
            </button>
          ))}
        </div>
        <button className={clsx("chip", grouped && "is-on")} aria-pressed={grouped} onClick={() => setGrouped((g) => !g)}>
          {t("timeline.groupByWorktree")}
        </button>
      </div>
      {query && <p className="filter-note">{t("timeline.searchNote", { query, count: snaps.data?.length ?? 0 })}</p>}

      <div className="timeline-scroll" ref={scrollRef}>
        {error && <ErrorBlock error={error} />}
        {loading && <SkeletonRows rows={8} />}
        {!loading && items.length === 0 && (
          <EmptyState compact title={t("timeline.empty.title")} body={query ? t("timeline.empty.search") : t("timeline.empty.body")} />
        )}
        <div className="timeline-list" style={{ height: virt.getTotalSize() }} role="list">
          {virt.getVirtualItems().map((vi) => {
            const it = items[vi.index];
            return (
              <div
                key={it.key}
                data-index={vi.index}
                ref={virt.measureElement}
                className="tl-slot"
                style={{ transform: `translateY(${vi.start}px)` }}
                role="listitem"
              >
                {it.type === "day" && <div className="tl-day">{formatDay(it.at, locale)}</div>}
                {it.type === "group" && (
                  <div className="tl-day mono" title={it.path}>
                    {it.path ? baseName(it.path) : t("timeline.projectWide")}
                  </div>
                )}
                {it.type === "snapshot" && (
                  <SnapshotRow s={it.s} selected={selectedSnapshot === it.s.id} onClick={() => nav.select({ type: "snapshot", id: it.s.id })} />
                )}
                {it.type === "event" && (
                  <EventRow
                    e={it.e}
                    selected={selectedEvent === it.e.id}
                    onClick={() =>
                      it.e.snapshotId
                        ? nav.select({ type: "snapshot", id: it.e.snapshotId })
                        : nav.select({ type: "event", id: it.e.id })
                    }
                  />
                )}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

function SnapshotRow({ s, selected, onClick }: { s: SnapshotSummary; selected: boolean; onClick: () => void }) {
  const { t, locale } = useI18n();
  return (
    <button className={clsx("tl-row", selected && "is-selected")} onClick={onClick}>
      <span className="tl-time mono">{formatTime(s.createdAt, locale)}</span>
      <span className={clsx("tl-glyph snap", `tone-${snapshotTone(s.status)}`, `trigger-${s.trigger}`)} aria-hidden />
      <span className="tl-main">
        <span className="tl-title">
          {s.note || t(`snapshot.title.${s.trigger}` as "snapshot.title.manual")}
          {s.pinned && <Pin size={12} className="tl-pin" aria-label={t("snapshot.pinned")} />}
        </span>
        <span className="tl-sub">
          <span className="mono">{s.branch ?? (s.detached ? t("common.detached") : "—")}</span>
          <span>{baseName(s.worktreePath)}</span>
          <span>{t("snapshot.filesCount", { count: s.entryCount })}</span>
          {s.skippedCount + s.failedCount > 0 && (
            <span className="tl-warn">{t("snapshot.notCapturedCount", { count: s.skippedCount + s.failedCount })}</span>
          )}
        </span>
      </span>
      <span className={clsx("tl-status", `tone-${snapshotTone(s.status)}`)}>{t(`snapshot.status.${s.status}` as "snapshot.status.verified")}</span>
      <span className="tl-size mono">{s.newBytes > 0 ? `+${formatBytes(s.newBytes)}` : "±0"}</span>
    </button>
  );
}

function EventRow({ e, selected, onClick }: { e: TimelineEvent; selected: boolean; onClick: () => void }) {
  const { td, locale } = useI18n();
  const look = describeEvent(e, td);
  return (
    <button className={clsx("tl-row tl-event", selected && "is-selected")} onClick={onClick}>
      <span className="tl-time mono">{formatTime(e.at, locale)}</span>
      <span className={clsx("tl-glyph ev", `tone-${look.tone}`)}>{look.icon}</span>
      <span className="tl-main">
        <span className="tl-title">{look.title}</span>
      </span>
    </button>
  );
}
