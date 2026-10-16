import { useMemo, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { FlaskConical, FolderPlus, ShieldCheck } from "lucide-react";
import clsx from "clsx";
import { useI18n } from "../../lib/i18n";
import { useEvents, useProjects, useRisks, useSettings, useSnapshots, useWorktrees, keys } from "../../lib/queries";
import { formatRelative, pathTail } from "../../lib/format";
import { protectionState, severityTone } from "../../lib/protection";
import { describeEvent } from "../../lib/describe";
import { api } from "../../lib/ipc";
import type { Project } from "../../lib/types";
import { Badge, Button, EmptyState, ErrorBlock, Segmented, Skeleton, useToast } from "../../components/ui";
import { useNav } from "../../app/nav";
import { RecorderStrip } from "./RecorderStrip";
import { useNow } from "../../lib/useNow";
import { Topology } from "./Topology";

const HOUR = 3_600_000;

export function Overview({ project, loading, onAdd }: { project: Project | undefined; loading: boolean; onAdd: () => void }) {
  if (loading) {
    return (
      <div className="page">
        <Skeleton w={420} h={30} />
        <div style={{ height: 24 }} />
        <Skeleton h={120} />
      </div>
    );
  }
  if (!project) return <NoProjects onAdd={onAdd} />;
  return <ProjectOverview project={project} />;
}

function NoProjects({ onAdd }: { onAdd: () => void }) {
  const { t } = useI18n();
  const nav = useNav();
  const qc = useQueryClient();
  const toast = useToast();
  const [building, setBuilding] = useState(false);
  const openDemo = async () => {
    setBuilding(true);
    try {
      const p = await api.buildDemo();
      await qc.invalidateQueries({ queryKey: keys.projects });
      nav.setProject(p.id);
    } catch (e) {
      toast({ tone: "red", title: t("demo.failed"), body: (e as Error).message });
    } finally {
      setBuilding(false);
    }
  };
  return (
    <div className="page">
      <EmptyState
        icon={<ShieldCheck size={22} />}
        title={t("overview.empty.title")}
        body={t("overview.empty.body")}
        action={
          <>
            <Button variant="primary" icon={<FolderPlus size={15} />} onClick={onAdd}>
              {t("switcher.add")}
            </Button>
            <Button icon={<FlaskConical size={15} />} onClick={openDemo} loading={building}>
              {t("demo.open")}
            </Button>
          </>
        }
      />
    </div>
  );
}

function ProjectOverview({ project }: { project: Project }) {
  const { t, tn, td, locale } = useI18n();
  const nav = useNav();
  const settings = useSettings();
  const projects = useProjects();
  const [range, setRange] = useState<"1h" | "24h" | "7d">("24h");
  const now = useNow();
  const since = now - (range === "1h" ? HOUR : range === "24h" ? 24 * HOUR : 7 * 24 * HOUR);
  const worktrees = useWorktrees(project.id);
  const snapshots = useSnapshots({ projectId: project.id, since, limit: 3000 });
  const events = useEvents({ projectId: project.id, since, limit: 3000 });
  const risks = useRisks(project.id);
  const stale = settings.data?.staleProtectionMinutes ?? 120;

  const states = useMemo(
    () => (worktrees.data ?? []).map((w) => ({ w, state: protectionState(w, project, stale, now) })),
    [worktrees.data, project, stale, now],
  );
  const count = (s: string) => states.filter((x) => x.state === s).length;
  const missing = count("missing");
  const exposed = count("unprotected") + count("stale");
  const prot = count("protected");
  const dirty = states.filter((x) => x.w.dirty).length;
  const critical = (risks.data ?? []).filter((r) => r.severity === "critical").length;

  const statement =
    missing > 0
      ? tn("overview.statement.missing", missing)
      : exposed > 0
        ? tn("overview.statement.exposed", exposed)
        : dirty > 0
          ? t("overview.statement.allProtected")
          : t("overview.statement.clean");
  const statementTone = missing > 0 || critical > 0 ? "red" : exposed > 0 ? "amber" : "cyan";

  const realProjects = (projects.data ?? []).filter((p) => !p.isDemo).length;
  const active = states.filter((x) => x.w.diagnosis === "healthy" && !x.w.goneFromGit).length;
  const openRisks = (risks.data ?? []).filter((r) => r.severity !== "info");
  const recent = (events.data ?? []).filter((e) => !["riskResolved"].includes(e.kind)).slice(0, 9);

  if (worktrees.error) return <div className="page"><ErrorBlock error={worktrees.error} onRetry={() => worktrees.refetch()} /></div>;

  return (
    <div className="page overview">
      <header className="ov-header">
        <h1 className={clsx("ov-statement", `tone-${statementTone}`)}>{statement}</h1>
        <p className="ov-sub">
          {t("overview.sub", {
            protectedCount: prot,
            total: states.length,
            dirty,
          })}
        </p>
      </header>

      <dl className="fact-strip">
        <div className="fact">
          <dt>{t("overview.fact.repositories")}</dt>
          <dd className="num">{realProjects}</dd>
        </div>
        <div className="fact">
          <dt>{t("overview.fact.activeWorktrees")}</dt>
          <dd className="num">
            {active}
            <span className="fact-of">/{states.length}</span>
          </dd>
        </div>
        <div className="fact">
          <dt>{t("overview.fact.lastVerified")}</dt>
          <dd>{project.lastVerifiedSnapshotAt ? formatRelative(project.lastVerifiedSnapshotAt, locale) : t("common.never")}</dd>
        </div>
        <div className="fact">
          <dt>{t("overview.fact.recentChanges")}</dt>
          <dd className="num">{dirty > 0 ? tn("overview.fact.dirtyWorktrees", dirty) : t("overview.fact.noChanges")}</dd>
        </div>
        <div className={clsx("fact", openRisks.length > 0 && "fact-alert")}>
          <dt>{t("overview.fact.attention")}</dt>
          <dd className="num">
            <button className="fact-link" onClick={() => nav.go("risks")} disabled={openRisks.length === 0}>
              {openRisks.length}
            </button>
          </dd>
        </div>
      </dl>

      <section className="section">
        <div className="section-head">
          <div>
            <h2 className="section-title">{t("overview.recorder.title")}</h2>
            <p className="section-note">{t("overview.recorder.note")}</p>
          </div>
          <Segmented
            label={t("overview.recorder.range")}
            size="sm"
            value={range}
            onChange={setRange}
            options={[
              { value: "1h", label: t("range.1h") },
              { value: "24h", label: t("range.24h") },
              { value: "7d", label: t("range.7d") },
            ]}
          />
        </div>
        {worktrees.isLoading || snapshots.isLoading ? (
          <Skeleton h={160} />
        ) : (
          <RecorderStrip
            project={project}
            rows={states}
            snapshots={snapshots.data ?? []}
            events={events.data ?? []}
            start={since}
            end={now}
            onSelectSnapshot={(id) => nav.select({ type: "snapshot", id })}
            onSelectEvent={(id) => nav.select({ type: "event", id })}
            onSelectWorktree={(path) => nav.select({ type: "worktree", path })}
          />
        )}
      </section>

      <section className="section">
          <div className="section-head">
            <h2 className="section-title">{t("overview.map.title")}</h2>
            <button className="btn btn-quiet btn-sm" onClick={() => nav.go("worktrees")}>
              {t("overview.map.open")}
            </button>
          </div>
          <Topology
            project={project}
            rows={states}
            selected={nav.selection?.type === "worktree" ? nav.selection.path : null}
            onSelect={(path) => nav.select({ type: "worktree", path })}
          />
      </section>

      <div className="ov-grid">
          <section className="section">
            <div className="section-head">
              <h2 className="section-title">{t("overview.attention.title")}</h2>
            </div>
            {openRisks.length === 0 ? (
              <p className="quiet-note">{t("overview.attention.none")}</p>
            ) : (
              <ul className="mini-list">
                {openRisks.slice(0, 4).map((r) => (
                  <li key={r.id}>
                    <button className="mini-row" onClick={() => nav.select({ type: "risk", id: r.id })}>
                      <Badge tone={severityTone(r.severity)}>{t(`severity.${r.severity}` as "severity.info")}</Badge>
                      <span className="mini-text">
                        <span className="mini-title">{td(`risk.${r.rule}.title`, r.rule)}</span>
                        {r.worktreePath && <span className="mini-sub mono">{pathTail(r.worktreePath, 2)}</span>}
                      </span>
                      <span className="mini-when">{formatRelative(r.detectedAt, locale)}</span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </section>

          <section className="section">
            <div className="section-head">
              <h2 className="section-title">{t("overview.recent.title")}</h2>
              <button className="btn btn-quiet btn-sm" onClick={() => nav.go("timeline")}>
                {t("overview.recent.open")}
              </button>
            </div>
            {recent.length === 0 ? (
              <p className="quiet-note">{t("overview.recent.none")}</p>
            ) : (
              <ul className="mini-list">
                {recent.map((e) => {
                  const look = describeEvent(e, td);
                  return (
                    <li key={e.id}>
                      <button
                        className="mini-row"
                        onClick={() =>
                          e.snapshotId ? nav.select({ type: "snapshot", id: e.snapshotId }) : nav.select({ type: "event", id: e.id })
                        }
                      >
                        <span className={clsx("mini-icon", `tone-${look.tone}`)}>{look.icon}</span>
                        <span className="mini-text">
                          <span className="mini-title">{look.title}</span>
                        </span>
                        <span className="mini-when">{formatRelative(e.at, locale)}</span>
                      </button>
                    </li>
                  );
                })}
              </ul>
            )}
          </section>
      </div>
    </div>
  );
}
