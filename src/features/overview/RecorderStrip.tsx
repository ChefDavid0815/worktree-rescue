import clsx from "clsx";
import { useMemo } from "react";
import { useI18n } from "../../lib/i18n";
import { formatDateTime, formatDay, formatTime, pathTail, samePath } from "../../lib/format";
import { describeEvent } from "../../lib/describe";
import type { Project, SnapshotSummary, TimelineEvent, Worktree } from "../../lib/types";
import { ProtectionRing, type RingState } from "../../components/ui";

const HOUR = 3_600_000;

interface Row {
  w: Worktree;
  state: RingState;
}

/**
 * The "flight recorder": one track per worktree over the selected window.
 * Ticks are real snapshots (height = trigger, colour = status); markers are
 * real Git/filesystem events; shaded spans show exposure (changes newer than
 * the last capture) and unreachability. Nothing here is interpolated.
 */
export function RecorderStrip({
  rows,
  snapshots,
  events,
  start,
  end,
  onSelectSnapshot,
  onSelectEvent,
  onSelectWorktree,
}: {
  project: Project;
  rows: Row[];
  snapshots: SnapshotSummary[];
  events: TimelineEvent[];
  start: number;
  end: number;
  onSelectSnapshot: (id: string) => void;
  onSelectEvent: (id: number) => void;
  onSelectWorktree: (path: string) => void;
}) {
  const { t, td, locale } = useI18n();
  const span = end - start;
  const pct = (ms: number) => `${Math.max(0, Math.min(1, (ms - start) / span)) * 100}%`;

  const axis = useMemo(() => {
    const ticks: { at: number; label: string; major: boolean }[] = [];
    const step = span <= 2 * HOUR ? 10 * 60_000 : span <= 30 * HOUR ? 3 * HOUR : 24 * HOUR;
    const offset = new Date().getTimezoneOffset() * 60_000;
    const first = Math.ceil((start - offset) / step) * step + offset;
    for (let at = first; at < end; at += step) {
      const midnight = new Date(at).getHours() === 0;
      ticks.push({
        at,
        label: step >= 24 * HOUR || midnight ? formatDay(at, locale) : formatTime(at, locale),
        major: midnight,
      });
    }
    return ticks;
  }, [start, end, span, locale]);

  const markerKinds = new Set(["branchChanged", "headMoved", "worktreeUnreachable", "worktreeReappeared", "worktreeRemovedFromGit", "recoveryCompleted", "recoveryFailed"]);

  return (
    <div className="recorder" role="group" aria-label={t("overview.recorder.title")}>
      <div className="rec-axis" aria-hidden>
        <span className="rec-label-col" />
        <div className="rec-axis-track">
          {axis.map((a) => (
            <span key={a.at} className={clsx("rec-axis-tick", a.major && "is-major")} style={{ left: pct(a.at) }}>
              <span>{a.label}</span>
            </span>
          ))}
          <span className="rec-now" style={{ left: "100%" }}>
            <span>{t("common.now")}</span>
          </span>
        </div>
      </div>
      {rows.map(({ w, state }) => {
        const snaps = snapshots.filter((s) => samePath(s.worktreePath, w.path));
        const evs = events.filter((e) => e.worktreePath && samePath(e.worktreePath, w.path) && markerKinds.has(e.kind));
        const unreachableSince = !(w.diagnosis === "healthy" && !w.goneFromGit)
          ? (evs.find((e) => e.kind === "worktreeUnreachable" || e.kind === "worktreeRemovedFromGit")?.at ?? w.lastSeenAt ?? start)
          : null;
        const exposedFrom =
          (state === "stale" || state === "unprotected") && w.lastDirtyAt ? Math.max(w.lastProtectedAt ?? w.lastDirtyAt, start) : null;
        return (
          <div key={w.id} className={clsx("rec-row", `state-${state}`)}>
            <button className="rec-label-col rec-label" onClick={() => onSelectWorktree(w.path)} title={w.path}>
              <ProtectionRing state={state} size={20} />
              <span className="rec-name">
                <span className="rec-branch mono">{w.branch ?? (w.detached ? t("common.detached") : "—")}</span>
                <span className="rec-path">{w.isMain ? t("worktree.main") : pathTail(w.path, 1)}</span>
              </span>
            </button>
            <div className="rec-track">
              <span className="rec-base" />
              {exposedFrom !== null && (
                <span
                  className="rec-span rec-exposed"
                  style={{ left: pct(exposedFrom), right: 0 }}
                  title={t("overview.recorder.exposed")}
                />
              )}
              {unreachableSince !== null && (
                <span
                  className="rec-span rec-unreachable"
                  style={{ left: pct(Math.max(unreachableSince, start)), right: 0 }}
                  title={td(`diagnosis.${w.diagnosis}`, w.diagnosis)}
                />
              )}
              {snaps.map((s) => (
                <button
                  key={s.id}
                  className={clsx("rec-tick", `status-${s.status}`, `trigger-${s.trigger}`)}
                  style={{ left: pct(s.createdAt) }}
                  onClick={() => onSelectSnapshot(s.id)}
                  title={`${formatDateTime(s.createdAt, locale)}\n${t(`snapshot.status.${s.status}` as "snapshot.status.verified")} · ${t(
                    `snapshot.trigger.${s.trigger}` as "snapshot.trigger.manual",
                  )}\n${t("snapshot.filesCount", { count: s.entryCount })}`}
                  aria-label={`${t("inspector.snapshot")} ${formatDateTime(s.createdAt, locale)}`}
                />
              ))}
              {evs.map((e) => {
                const look = describeEvent(e, td);
                return (
                  <button
                    key={e.id}
                    className={clsx("rec-marker", `kind-${e.kind}`, `tone-${look.tone}`)}
                    style={{ left: pct(e.at) }}
                    onClick={() => onSelectEvent(e.id)}
                    title={`${formatDateTime(e.at, locale)}\n${look.title}`}
                    aria-label={look.title}
                  />
                );
              })}
            </div>
          </div>
        );
      })}
      <div className="rec-legend" aria-hidden>
        <span><i className="lg-tick status-verified" />{t("legend.verified")}</span>
        <span><i className="lg-tick status-verified trigger-auto" />{t("legend.auto")}</span>
        <span><i className="lg-tick status-partial" />{t("legend.partial")}</span>
        <span><i className="lg-tick status-failed" />{t("legend.failed")}</span>
        <span><i className="lg-marker" />{t("legend.gitEvent")}</span>
        <span><i className="lg-span exposed" />{t("legend.exposed")}</span>
        <span><i className="lg-span unreachable" />{t("legend.unreachable")}</span>
      </div>
      {rows.length === 0 && <p className="quiet-note">{t("overview.recorder.noWorktrees")}</p>}
    </div>
  );
}
