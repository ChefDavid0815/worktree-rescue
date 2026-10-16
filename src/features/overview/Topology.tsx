import clsx from "clsx";
import { Lock } from "lucide-react";
import { useI18n } from "../../lib/i18n";
import { formatRelative, pathTail, samePath } from "../../lib/format";
import { changedCount } from "../../lib/protection";
import type { Project, Worktree } from "../../lib/types";
import { ProtectionRing, type RingState } from "../../components/ui";

const NODE_H = 66;
const GAP = 10;
const LINK_W = 72;

/**
 * Worktree relationship map built from `git worktree list`: the main
 * worktree on the left, each linked worktree branching off it. Connector
 * style follows the linked worktree's protection state.
 */
export function Topology({
  rows,
  selected,
  onSelect,
}: {
  project: Project;
  rows: { w: Worktree; state: RingState }[];
  selected: string | null;
  onSelect: (path: string) => void;
}) {
  const { t } = useI18n();
  const main = rows.find((r) => r.w.isMain);
  const linked = rows.filter((r) => !r.w.isMain);
  if (!main) return <p className="quiet-note">{t("overview.recorder.noWorktrees")}</p>;
  const height = Math.max(NODE_H, linked.length * (NODE_H + GAP) - GAP);
  const mainY = height / 2;
  return (
    <div className="topology" style={{ minHeight: height }}>
      <div className="topo-main" style={{ top: mainY - NODE_H / 2 }}>
        <Node row={main} selected={!!selected && samePath(selected, main.w.path)} onSelect={onSelect} />
      </div>
      {linked.length > 0 && (
        <svg className="topo-links" width={LINK_W} height={height} aria-hidden>
          {linked.map((r, i) => {
            const y = i * (NODE_H + GAP) + NODE_H / 2;
            return (
              <path
                key={r.w.id}
                className={clsx("topo-link", `state-${r.state}`)}
                d={`M0 ${mainY} C ${LINK_W * 0.55} ${mainY}, ${LINK_W * 0.45} ${y}, ${LINK_W} ${y}`}
              />
            );
          })}
          <circle cx="2" cy={mainY} r="3" className="topo-root" />
        </svg>
      )}
      <div className="topo-linked">
        {linked.map((r) => (
          <Node key={r.w.id} row={r} selected={!!selected && samePath(selected, r.w.path)} onSelect={onSelect} />
        ))}
        {linked.length === 0 && <p className="quiet-note topo-none">{t("overview.map.noLinked")}</p>}
      </div>
    </div>
  );
}

function Node({ row, selected, onSelect }: { row: { w: Worktree; state: RingState }; selected: boolean; onSelect: (p: string) => void }) {
  const { t, td, tn, locale } = useI18n();
  const { w, state } = row;
  const n = changedCount(w);
  const status =
    state === "missing" || state === "unknown"
      ? td(`diagnosis.${w.diagnosis}`, w.diagnosis)
      : w.dirty
        ? tn("worktree.changes", n)
        : t("worktree.clean");
  return (
    <button
      className={clsx("topo-node", `state-${state}`, selected && "is-selected")}
      style={{ height: NODE_H }}
      onClick={() => onSelect(w.path)}
      title={w.path}
    >
      <ProtectionRing state={state} size={26} title={t(`ring.${state}` as "ring.protected")} />
      <span className="tn-text">
        <span className="tn-branch mono">
          {w.branch ?? t("common.detached")}
          {w.locked && <Lock size={11} className="tn-lock" aria-label={t("worktree.locked")} />}
        </span>
        <span className="tn-path">{w.isMain ? t("worktree.main") : pathTail(w.path, 1)}</span>
      </span>
      <span className="tn-status">
        <span className={clsx("tn-state", `state-${state}`)}>{status}</span>
        <span className="tn-when">
          {w.lastProtectedAt ? formatRelative(w.lastProtectedAt, locale) : t("worktree.neverCaptured")}
        </span>
      </span>
    </button>
  );
}
