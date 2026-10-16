import clsx from "clsx";
import { AnimatePresence, motion } from "motion/react";
import {
  Camera,
  ChevronDown,
  FlaskConical,
  GitFork,
  History,
  LifeBuoy,
  PanelRightClose,
  PanelRightOpen,
  Plus,
  Radar,
  RefreshCw,
  Settings2,
  ShieldAlert,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useI18n } from "../lib/i18n";
import { api } from "../lib/ipc";
import { keys, useAutoActivity, useProjects, useSettings, useWorktrees } from "../lib/queries";
import { protectionState } from "../lib/protection";
import { formatRelative, pathTail } from "../lib/format";
import type { Project } from "../lib/types";
import { Badge, Button, IconButton, Popover, ProtectionRing, useToast } from "../components/ui";
import { LogoMark } from "../components/LogoMark";
import { useNav, type View } from "./nav";
import { Overview } from "../features/overview/Overview";
import { WorktreesView } from "../features/worktrees/WorktreesView";
import { TimelineView } from "../features/snapshots/TimelineView";
import { RecoveryView } from "../features/recovery/RecoveryView";
import { RisksView } from "../features/risks/RisksView";
import { SettingsView } from "../features/settings/SettingsView";
import { Inspector } from "./Inspector";
import { CaptureDialog } from "../features/snapshots/CaptureDialog";
import { AddRepositoryDialog } from "../features/onboarding/AddRepositoryDialog";

const NAV: { view: View; icon: ReactNode; key: "nav.overview" | "nav.worktrees" | "nav.timeline" | "nav.recovery" | "nav.risks" }[] = [
  { view: "overview", icon: <Radar size={19} strokeWidth={1.7} />, key: "nav.overview" },
  { view: "worktrees", icon: <GitFork size={19} strokeWidth={1.7} />, key: "nav.worktrees" },
  { view: "timeline", icon: <History size={19} strokeWidth={1.7} />, key: "nav.timeline" },
  { view: "recovery", icon: <LifeBuoy size={19} strokeWidth={1.7} />, key: "nav.recovery" },
  { view: "risks", icon: <ShieldAlert size={19} strokeWidth={1.7} />, key: "nav.risks" },
];

export function Shell() {
  const { t } = useI18n();
  const nav = useNav();
  const projects = useProjects();
  const [captureOpen, setCaptureOpen] = useState(false);
  const [addOpen, setAddOpen] = useState(false);

  // Keep a valid project selected.
  useEffect(() => {
    const list = projects.data;
    if (!list) return;
    if (list.length === 0) {
      if (nav.projectId) nav.setProject(null);
      return;
    }
    if (!nav.projectId || !list.some((p) => p.id === nav.projectId)) {
      nav.setProject((list.find((p) => !p.isDemo) ?? list[0]).id);
    }
  }, [projects.data, nav]);

  const project = projects.data?.find((p) => p.id === nav.projectId);

  // Each view starts at the top.
  useEffect(() => {
    document.getElementById("workspace")?.scrollTo?.({ top: 0 });
  }, [nav.view, nav.projectId]);

  // Keyboard: Ctrl+1..6 switches views, Ctrl+Shift+S creates a snapshot.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey)) return;
      const views: View[] = ["overview", "worktrees", "timeline", "recovery", "risks", "settings"];
      const n = Number(e.key);
      if (n >= 1 && n <= 6) {
        e.preventDefault();
        nav.go(views[n - 1]);
      } else if (e.shiftKey && e.key.toLowerCase() === "s" && project) {
        e.preventDefault();
        setCaptureOpen(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [nav, project]);

  const openRisks = project ? project.openRisks.attention + project.openRisks.critical : 0;

  return (
    <div className={clsx("shell", nav.inspectorOpen && project && "has-inspector")}>
      <nav className="rail" aria-label={t("nav.label")}>
        <div className="rail-logo" title="Worktree Rescue">
          <LogoMark size={34} />
        </div>
        <div className="rail-items">
          {NAV.map((n) => (
            <button
              key={n.view}
              className={clsx("rail-item", nav.view === n.view && "is-active")}
              onClick={() => nav.go(n.view)}
              aria-current={nav.view === n.view ? "page" : undefined}
            >
              <span className="rail-icon">
                {n.icon}
                {n.view === "risks" && openRisks > 0 && (
                  <span className={clsx("rail-count", project!.openRisks.critical > 0 && "is-critical")} aria-hidden>
                    {openRisks}
                  </span>
                )}
              </span>
              <span className="rail-label">{t(n.key)}</span>
            </button>
          ))}
        </div>
        <div className="rail-bottom">
          <button
            className={clsx("rail-item", nav.view === "settings" && "is-active")}
            onClick={() => nav.go("settings")}
            aria-current={nav.view === "settings" ? "page" : undefined}
          >
            <span className="rail-icon">
              <Settings2 size={19} strokeWidth={1.7} />
            </span>
            <span className="rail-label">{t("nav.settings")}</span>
          </button>
        </div>
      </nav>

      <div className="main">
        <TopBar
          project={project}
          projects={projects.data ?? []}
          onCapture={() => setCaptureOpen(true)}
          onAdd={() => setAddOpen(true)}
        />
        {project?.isDemo && (
          <div className="demo-banner" role="note">
            <FlaskConical size={14} />
            <span>{t("demo.banner")}</span>
          </div>
        )}
        <main className="workspace" id="workspace">
          {/* Keyed remount with a transform-only entrance: content is never invisible,
              even if the WebView pauses animation frames while occluded. */}
          <div key={nav.view + (project?.id ?? "")} className="workspace-view view-enter">
            <ViewSwitch project={project} loading={projects.isLoading} onAdd={() => setAddOpen(true)} />
          </div>
        </main>
      </div>

      {project && nav.inspectorOpen && <Inspector project={project} />}

      {project && <CaptureDialog open={captureOpen} onClose={() => setCaptureOpen(false)} project={project} />}
      <AddRepositoryDialog open={addOpen} onClose={() => setAddOpen(false)} />
    </div>
  );
}

function ViewSwitch({ project, loading, onAdd }: { project: Project | undefined; loading: boolean; onAdd: () => void }) {
  const nav = useNav();
  if (nav.view === "settings") return <SettingsView project={project} />;
  if (!project) return <Overview project={undefined} loading={loading} onAdd={onAdd} />;
  switch (nav.view) {
    case "worktrees":
      return <WorktreesView project={project} />;
    case "timeline":
      return <TimelineView project={project} />;
    case "recovery":
      return <RecoveryView project={project} />;
    case "risks":
      return <RisksView project={project} />;
    default:
      return <Overview project={project} loading={false} onAdd={onAdd} />;
  }
}

function TopBar({
  project,
  projects,
  onCapture,
  onAdd,
}: {
  project: Project | undefined;
  projects: Project[];
  onCapture: () => void;
  onAdd: () => void;
}) {
  const { t, locale } = useI18n();
  const nav = useNav();
  const qc = useQueryClient();
  const toast = useToast();
  const switcherRef = useRef<HTMLButtonElement>(null);
  const [switcherOpen, setSwitcherOpen] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const activity = useAutoActivity();

  const refresh = async () => {
    if (!project) return;
    setRefreshing(true);
    try {
      await api.refreshProject(project.id);
      await qc.invalidateQueries();
    } catch (e) {
      toast({ tone: "red", title: t("toast.refreshFailed"), body: String((e as Error).message ?? e) });
    } finally {
      setRefreshing(false);
    }
  };

  const toggleAuto = async () => {
    if (!project) return;
    try {
      await api.setAutoProtect(project.id, !project.autoProtect);
      await qc.invalidateQueries({ queryKey: keys.projects });
      toast({
        tone: project.autoProtect ? "neutral" : "cyan",
        title: project.autoProtect ? t("toast.autoOff") : t("toast.autoOn"),
      });
    } catch (e) {
      toast({ tone: "red", title: t("toast.actionFailed"), body: String((e as Error).message ?? e) });
    }
  };

  return (
    <header className="topbar">
      <button
        ref={switcherRef}
        className="project-switcher"
        onClick={() => setSwitcherOpen((o) => !o)}
        aria-haspopup="menu"
        aria-expanded={switcherOpen}
      >
        <span className="ps-name">{project?.name ?? t("topbar.noProject")}</span>
        {project && <span className="ps-path mono">{pathTail(project.rootPath, 2)}</span>}
        <ChevronDown size={15} className="ps-chevron" />
      </button>
      <Popover anchor={switcherRef.current} open={switcherOpen} onClose={() => setSwitcherOpen(false)} width={380}>
        <ProjectMenu
          projects={projects}
          current={project?.id}
          onPick={(id) => {
            nav.setProject(id);
            setSwitcherOpen(false);
          }}
          onAdd={() => {
            setSwitcherOpen(false);
            onAdd();
          }}
        />
      </Popover>

      {project && (
        <button
          className={clsx("protect-pill", project.autoProtect && project.monitoring === "active" ? "is-on" : "is-off")}
          onClick={toggleAuto}
          title={project.autoProtect ? t("topbar.autoOnHint") : t("topbar.autoOffHint")}
          aria-pressed={project.autoProtect}
        >
          <span className="pp-dot" aria-hidden />
          {project.autoProtect ? t("topbar.autoOn") : t("topbar.autoOff")}
        </button>
      )}

      <AnimatePresence>
        {activity && (
          <motion.span className="activity" initial={{ x: -4 }} animate={{ x: 0 }} exit={{ opacity: 0 }} role="status">
            <span className="activity-bar" />
            {t(`capture.phase.${activity.phase}` as "capture.phase.scanning")}
          </motion.span>
        )}
      </AnimatePresence>

      <div className="topbar-spacer" />
      {project && (
        <span className="topbar-meta">
          {project.lastVerifiedSnapshotAt
            ? t("topbar.lastVerified", { when: formatRelative(project.lastVerifiedSnapshotAt, locale) })
            : t("topbar.noVerified")}
        </span>
      )}
      {project && (
        <IconButton
          label={t("topbar.refresh")}
          icon={<RefreshCw size={16} className={refreshing ? "spin" : undefined} />}
          onClick={refresh}
          disabled={refreshing}
        />
      )}
      {project && (
        <Button variant="primary" icon={<Camera size={15} />} onClick={onCapture} title={`${t("topbar.capture")} (Ctrl+Shift+S)`}>
          {t("topbar.capture")}
        </Button>
      )}
      {project && (
        <IconButton
          label={nav.inspectorOpen ? t("topbar.hideInspector") : t("topbar.showInspector")}
          icon={nav.inspectorOpen ? <PanelRightClose size={17} /> : <PanelRightOpen size={17} />}
          onClick={() => nav.setInspectorOpen(!nav.inspectorOpen)}
        />
      )}
    </header>
  );
}

function ProjectMenu({
  projects,
  current,
  onPick,
  onAdd,
}: {
  projects: Project[];
  current: string | undefined;
  onPick: (id: string) => void;
  onAdd: () => void;
}) {
  const { t, locale } = useI18n();
  const real = projects.filter((p) => !p.isDemo);
  const demo = projects.filter((p) => p.isDemo);
  return (
    <div className="project-menu" role="menu">
      {real.length > 0 && <p className="menu-heading">{t("switcher.repositories")}</p>}
      {real.map((p) => (
        <ProjectMenuItem key={p.id} p={p} active={p.id === current} onPick={onPick} locale={locale} />
      ))}
      {demo.length > 0 && <p className="menu-heading">{t("switcher.sandbox")}</p>}
      {demo.map((p) => (
        <ProjectMenuItem key={p.id} p={p} active={p.id === current} onPick={onPick} locale={locale} />
      ))}
      <div className="menu-sep" />
      <button className="menu-item" role="menuitem" onClick={onAdd}>
        <Plus size={15} />
        <span>{t("switcher.add")}</span>
      </button>
    </div>
  );
}

function ProjectMenuItem({
  p,
  active,
  onPick,
  locale,
}: {
  p: Project;
  active: boolean;
  onPick: (id: string) => void;
  locale: "en" | "zh-CN";
}) {
  const { t } = useI18n();
  const settings = useSettings();
  const wts = useWorktrees(p.id);
  const ring = useMemo(() => {
    const states = (wts.data ?? []).map((w) => protectionState(w, p, settings.data?.staleProtectionMinutes ?? 120));
    if (states.includes("missing")) return "missing" as const;
    if (states.includes("unprotected")) return "unprotected" as const;
    if (states.includes("stale")) return "stale" as const;
    if (p.monitoring === "paused") return "paused" as const;
    return states.includes("protected") ? ("protected" as const) : ("clean" as const);
  }, [wts.data, p, settings.data]);
  return (
    <button className={clsx("menu-item project-item", active && "is-active")} role="menuitem" onClick={() => onPick(p.id)}>
      <ProtectionRing state={ring} size={22} />
      <span className="pi-text">
        <span className="pi-name">
          {p.name}
          {p.monitoring === "paused" && <Badge>{t("common.paused")}</Badge>}
        </span>
        <span className="pi-sub mono">{pathTail(p.rootPath, 3)}</span>
      </span>
      <span className="pi-when">{p.lastSnapshotAt ? formatRelative(p.lastSnapshotAt, locale) : "—"}</span>
    </button>
  );
}
