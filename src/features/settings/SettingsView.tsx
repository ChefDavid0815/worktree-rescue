import clsx from "clsx";
import { FolderOpen, Download, RotateCcw, Save, Trash2, PauseCircle, PlayCircle, Eraser } from "lucide-react";
import { useEffect, useMemo, useState, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { useI18n } from "../../lib/i18n";
import { api, isTauri } from "../../lib/ipc";
import { keys, useAppInfo, useProjects, useSettings, useStorage } from "../../lib/queries";
import { formatBytes, formatDateTime } from "../../lib/format";
import type { Project, Settings } from "../../lib/types";
import { Badge, Button, Dialog, ErrorBlock, Field, PathText, Segmented, Skeleton, TextInput, Toggle, useToast } from "../../components/ui";

const MB = 1024 * 1024;
const GB = 1024 * MB;

const SECTIONS = ["protection", "scope", "storage", "recovery", "notifications", "appearance", "repositories", "privacy", "about"] as const;
type Section = (typeof SECTIONS)[number];

export function SettingsView({ project: _project }: { project: Project | undefined }) {
  const { t } = useI18n();
  const settings = useSettings();
  const qc = useQueryClient();
  const toast = useToast();
  const [draft, setDraft] = useState<Settings | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [active, setActive] = useState<Section>("protection");

  useEffect(() => {
    if (settings.data && !draft) setDraft(settings.data);
  }, [settings.data, draft]);

  const dirty = useMemo(() => !!draft && !!settings.data && JSON.stringify(draft) !== JSON.stringify(settings.data), [draft, settings.data]);

  if (settings.isLoading || !draft) return <div className="page"><Skeleton h={300} /></div>;

  const set = <K extends keyof Settings>(k: K, v: Settings[K]) => setDraft((d) => (d ? { ...d, [k]: v } : d));

  const persist = async (next: Settings, quiet = false) => {
    setSaving(true);
    setError(null);
    try {
      const saved = await api.updateSettings(next);
      qc.setQueryData(keys.settings, saved);
      setDraft(saved);
      if (!quiet) toast({ tone: "cyan", title: t("settings.saved") });
    } catch (e) {
      setError(e);
    } finally {
      setSaving(false);
    }
  };

  /** Appearance applies immediately (and keeps any other unsaved edits). */
  const applyNow = async <K extends "theme" | "language">(k: K, v: Settings[K]) => {
    const base = settings.data!;
    const saved = await api.updateSettings({ ...base, [k]: v });
    qc.setQueryData(keys.settings, saved);
    setDraft((d) => (d ? { ...d, [k]: v } : d));
  };

  const scrollTo = (s: Section) => {
    setActive(s);
    document.getElementById(`settings-${s}`)?.scrollIntoView({ behavior: "smooth", block: "start" });
  };

  return (
    <div className="settings-layout">
      <nav className="settings-nav" aria-label={t("settings.title")}>
        <h1 className="page-title">{t("settings.title")}</h1>
        {SECTIONS.map((s) => (
          <button key={s} className={clsx("settings-link", active === s && "is-active")} onClick={() => scrollTo(s)}>
            {t(`settings.section.${s}` as "settings.section.protection")}
          </button>
        ))}
      </nav>
      <div className="settings-body">
        <Block id="protection" title={t("settings.section.protection")} intro={t("settings.protection.intro")}>
          <Toggle
            checked={draft.autoProtectNewProjects}
            onChange={(v) => set("autoProtectNewProjects", v)}
            label={t("settings.autoNew")}
            description={t("settings.autoNew.desc")}
          />
          <div className="field-grid">
            <NumberField label={t("settings.quiet")} hint={t("settings.quiet.hint")} value={draft.quietPeriodSecs} unit="s" onChange={(v) => set("quietPeriodSecs", v)} min={5} max={3600} />
            <NumberField label={t("settings.minInterval")} hint={t("settings.minInterval.hint")} value={draft.minIntervalSecs} unit="s" onChange={(v) => set("minIntervalSecs", v)} min={30} max={86400} />
            <NumberField label={t("settings.checkpoint")} hint={t("settings.checkpoint.hint")} value={Math.round(draft.checkpointIntervalSecs / 60)} unit="min" onChange={(v) => set("checkpointIntervalSecs", v * 60)} min={1} max={1440} />
            <NumberField label={t("settings.stale")} hint={t("settings.stale.hint")} value={draft.staleProtectionMinutes} unit="min" onChange={(v) => set("staleProtectionMinutes", v)} min={5} max={10080} />
            <NumberField label={t("settings.poll")} hint={t("settings.poll.hint")} value={draft.inventoryPollSecs} unit="s" onChange={(v) => set("inventoryPollSecs", v)} min={15} max={3600} />
            <NumberField label={t("settings.massDeletion")} hint={t("settings.massDeletion.hint")} value={draft.massDeletionThreshold} unit={t("units.files")} onChange={(v) => set("massDeletionThreshold", v)} min={1} max={100000} />
          </div>
        </Block>

        <Block id="scope" title={t("settings.section.scope")} intro={t("settings.scope.intro")}>
          <Toggle checked={draft.includeUntracked} onChange={(v) => set("includeUntracked", v)} label={t("settings.untracked")} description={t("settings.untracked.desc")} />
          <Toggle checked={draft.includeSensitive} onChange={(v) => set("includeSensitive", v)} label={t("settings.sensitive")} description={t("settings.sensitive.desc")} />
          <div className="field-grid">
            <NumberField label={t("settings.maxFile")} hint={t("settings.maxFile.hint")} value={Math.round(draft.maxFileBytes / MB)} unit="MB" onChange={(v) => set("maxFileBytes", v * MB)} min={1} max={4096} />
          </div>
          <GlobField label={t("settings.exclude")} hint={t("settings.exclude.hint")} value={draft.excludeGlobs} onChange={(v) => set("excludeGlobs", v)} />
          <GlobField label={t("settings.sensitiveGlobs")} hint={t("settings.sensitiveGlobs.hint")} value={draft.sensitiveGlobs} onChange={(v) => set("sensitiveGlobs", v)} />
          <GlobField label={t("settings.includeIgnored")} hint={t("settings.includeIgnored.hint")} value={draft.includeIgnoredGlobs} onChange={(v) => set("includeIgnoredGlobs", v)} placeholder="target/keep/**" />
        </Block>

        <Block id="storage" title={t("settings.section.storage")} intro={t("settings.storage.intro")}>
          <div className="field-grid">
            <NumberField label={t("settings.budget")} hint={t("settings.budget.hint")} value={Math.round((draft.repoBudgetBytes / GB) * 10) / 10} step={0.5} unit="GB" onChange={(v) => set("repoBudgetBytes", Math.round(v * GB))} min={0.1} max={4096} />
            <NumberField label={t("settings.minFree")} hint={t("settings.minFree.hint")} value={Math.round((draft.minFreeSpaceBytes / GB) * 10) / 10} step={0.5} unit="GB" onChange={(v) => set("minFreeSpaceBytes", Math.round(v * GB))} min={0} max={4096} />
          </div>
          <h3 className="block-title">{t("settings.retention")}</h3>
          <p className="muted small">{t("settings.retention.desc")}</p>
          <div className="field-grid">
            <NumberField label={t("settings.keepAll")} value={draft.retention.keepAllHours} unit="h" onChange={(v) => set("retention", { ...draft.retention, keepAllHours: v })} min={1} max={8760} />
            <NumberField label={t("settings.keepHourly")} value={draft.retention.keepHourlyDays} unit={t("units.days")} onChange={(v) => set("retention", { ...draft.retention, keepHourlyDays: v })} min={0} max={365} />
            <NumberField label={t("settings.keepDaily")} value={draft.retention.keepDailyDays} unit={t("units.days")} onChange={(v) => set("retention", { ...draft.retention, keepDailyDays: v })} min={0} max={3650} />
            <NumberField label={t("settings.maxPerWorktree")} value={draft.retention.maxSnapshotsPerWorktree} unit={t("units.snapshots")} onChange={(v) => set("retention", { ...draft.retention, maxSnapshotsPerWorktree: v })} min={10} max={100000} />
          </div>
          <StoragePanel />
        </Block>

        <Block id="recovery" title={t("settings.section.recovery")} intro={t("settings.recovery.intro")}>
          <Field label={t("settings.recoveryDir")} hint={t("settings.recoveryDir.hint")}>
            <TextInput value={draft.recoveryDir} onChange={(e) => set("recoveryDir", e.target.value)} mono />
          </Field>
          <Field label={t("settings.gitPath")} hint={t("settings.gitPath.hint")}>
            <TextInput value={draft.gitPath ?? ""} onChange={(e) => set("gitPath", e.target.value.trim() || null)} placeholder={t("settings.gitPath.auto")} mono />
          </Field>
        </Block>

        <Block id="notifications" title={t("settings.section.notifications")}>
          <Toggle checked={draft.notifyOnRisk} onChange={(v) => set("notifyOnRisk", v)} label={t("settings.notifyRisk")} description={t("settings.notifyRisk.desc")} />
          <Toggle checked={draft.notifyOnFailure} onChange={(v) => set("notifyOnFailure", v)} label={t("settings.notifyFailure")} description={t("settings.notifyFailure.desc")} />
        </Block>

        <Block id="appearance" title={t("settings.section.appearance")}>
          <div className="inline-field">
            <span className="field-label">{t("settings.theme")}</span>
            <Segmented
              label={t("settings.theme")}
              value={settings.data!.theme}
              onChange={(v) => void applyNow("theme", v)}
              options={[
                { value: "system", label: t("settings.theme.system") },
                { value: "dark", label: t("settings.theme.dark") },
                { value: "light", label: t("settings.theme.light") },
              ]}
            />
          </div>
          <div className="inline-field">
            <span className="field-label">{t("settings.language")}</span>
            <Segmented
              label={t("settings.language")}
              value={settings.data!.language}
              onChange={(v) => void applyNow("language", v)}
              options={[
                { value: "auto", label: t("settings.language.auto") },
                { value: "en", label: "English" },
                { value: "zh-CN", label: "简体中文" },
              ]}
            />
          </div>
        </Block>

        <Block id="repositories" title={t("settings.section.repositories")} intro={t("settings.repositories.intro")}>
          <RepositoriesPanel />
        </Block>

        <Block id="privacy" title={t("settings.section.privacy")}>
          <ul className="promise-list">
            {(["offline", "noTelemetry", "noUpload", "noAccount", "readOnly"] as const).map((k) => (
              <li key={k}>{t(`privacy.${k}` as "privacy.offline")}</li>
            ))}
          </ul>
          <DiagnosticsPanel />
        </Block>

        <Block id="about" title={t("settings.section.about")}>
          <AboutPanel />
        </Block>
      </div>

      {(dirty || error !== null) && (
        <div className="save-bar" role="region" aria-label={t("settings.unsaved")}>
          {error !== null ? <ErrorBlock error={error} /> : <span>{t("settings.unsaved")}</span>}
          <div className="action-row">
            <Button variant="ghost" icon={<RotateCcw size={14} />} onClick={() => { setDraft(settings.data!); setError(null); }}>
              {t("settings.discard")}
            </Button>
            <Button
              variant="ghost"
              onClick={async () => {
                const d = await api.defaultSettings();
                setDraft({ ...d, theme: draft.theme, language: draft.language });
              }}
            >
              {t("settings.defaults")}
            </Button>
            <Button variant="primary" icon={<Save size={14} />} onClick={() => persist(draft)} loading={saving}>
              {t("settings.save")}
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

function Block({ id, title, intro, children }: { id: Section; title: string; intro?: string; children: ReactNode }) {
  return (
    <section className="settings-block" id={`settings-${id}`}>
      <h2 className="section-title">{title}</h2>
      {intro && <p className="settings-intro">{intro}</p>}
      <div className="settings-content">{children}</div>
    </section>
  );
}

function NumberField({
  label,
  hint,
  value,
  unit,
  onChange,
  min,
  max,
  step = 1,
}: {
  label: string;
  hint?: string;
  value: number;
  unit: string;
  onChange: (v: number) => void;
  min: number;
  max: number;
  step?: number;
}) {
  const [text, setText] = useState(String(value));
  useEffect(() => setText(String(value)), [value]);
  const invalid = Number.isNaN(Number(text)) || Number(text) < min || Number(text) > max;
  return (
    <Field label={label} hint={invalid ? `${min} – ${max}` : hint}>
      <div className={clsx("number-input", invalid && "is-invalid")}>
        <input
          className="input mono"
          inputMode="decimal"
          value={text}
          onChange={(e) => {
            setText(e.target.value);
            const n = Number(e.target.value);
            if (!Number.isNaN(n) && n >= min && n <= max) onChange(step < 1 ? n : Math.round(n));
          }}
          aria-invalid={invalid}
          aria-label={label}
        />
        <span className="unit">{unit}</span>
      </div>
    </Field>
  );
}

function GlobField({
  label,
  hint,
  value,
  onChange,
  placeholder,
}: {
  label: string;
  hint?: string;
  value: string[];
  onChange: (v: string[]) => void;
  placeholder?: string;
}) {
  const [text, setText] = useState(value.join("\n"));
  useEffect(() => setText(value.join("\n")), [value]);
  return (
    <Field label={label} hint={hint}>
      <textarea
        className="input mono"
        rows={Math.min(8, Math.max(3, value.length + 1))}
        value={text}
        placeholder={placeholder}
        spellCheck={false}
        onChange={(e) => {
          setText(e.target.value);
          onChange(
            e.target.value
              .split("\n")
              .map((s) => s.trim())
              .filter(Boolean),
          );
        }}
      />
    </Field>
  );
}

function StoragePanel() {
  const { t } = useI18n();
  const storage = useStorage();
  const qc = useQueryClient();
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  if (storage.isLoading) return <Skeleton h={120} />;
  if (storage.error) return <ErrorBlock error={storage.error} />;
  const s = storage.data!.main;
  const run = async () => {
    setBusy(true);
    try {
      const r = await api.runRetention();
      toast({
        tone: "cyan",
        title: t("storage.cleaned"),
        body: t("storage.cleanedBody", { snapshots: r.snapshotsPruned, blobs: r.blobsDeleted, bytes: formatBytes(r.bytesFreed) }),
      });
      await qc.invalidateQueries();
    } catch (e) {
      toast({ tone: "red", title: t("toast.actionFailed"), body: (e as Error).message });
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="storage-panel">
      <dl className="facts">
        <dt>{t("storage.dataDir")}</dt>
        <dd>
          <PathText path={s.dataDir} max={70} />
        </dd>
        <dt>{t("storage.content")}</dt>
        <dd className="num">{t("storage.contentLine", { blobs: s.blobCount, bytes: formatBytes(s.blobBytes), snapshots: s.snapshotCount })}</dd>
        <dt>{t("storage.free")}</dt>
        <dd className="num">{formatBytes(s.availableBytes)}</dd>
      </dl>
      <ul className="usage-list">
        {s.perProject.map((p) => {
          const ratio = p.budgetBytes > 0 ? p.referencedBytes / p.budgetBytes : 0;
          return (
            <li key={p.projectId}>
              <span className="usage-name">{p.name}</span>
              <span className="usage-bar">
                <span className={clsx("usage-fill", ratio > 0.9 ? "tone-red" : ratio > 0.7 ? "tone-amber" : "tone-cyan")} style={{ width: `${Math.min(100, ratio * 100)}%` }} />
              </span>
              <span className="usage-num mono">
                {formatBytes(p.referencedBytes)} / {formatBytes(p.budgetBytes)}
              </span>
            </li>
          );
        })}
      </ul>
      <div className="action-row">
        <Button icon={<Eraser size={14} />} onClick={run} loading={busy}>
          {t("storage.runRetention")}
        </Button>
        <Button variant="ghost" icon={<FolderOpen size={14} />} onClick={() => api.revealPath(s.dataDir)} disabled={!isTauri()}>
          {t("storage.openDataDir")}
        </Button>
      </div>
      <p className="muted small">{t("storage.demoSeparate", { bytes: formatBytes(storage.data!.demo.blobBytes) })}</p>
    </div>
  );
}

function RepositoriesPanel() {
  const { t } = useI18n();
  const projects = useProjects();
  const qc = useQueryClient();
  const toast = useToast();
  const [removing, setRemoving] = useState<Project | null>(null);
  const [deleting, setDeleting] = useState<Project | null>(null);
  const [confirmText, setConfirmText] = useState("");
  const list = projects.data ?? [];

  const wrap = async (fn: () => Promise<unknown>, ok: string) => {
    try {
      await fn();
      await qc.invalidateQueries();
      toast({ tone: "cyan", title: ok });
    } catch (e) {
      toast({ tone: "red", title: t("toast.actionFailed"), body: (e as Error).message });
    }
  };

  if (list.length === 0) return <p className="quiet-note">{t("settings.repositories.none")}</p>;
  return (
    <>
      <ul className="repo-list">
        {list.map((p) => (
          <li key={p.id} className="repo-row">
            <div className="repo-main">
              <span className="repo-name">
                {p.name}
                {p.isDemo && <Badge tone="violet">{t("common.demo")}</Badge>}
                {p.monitoring === "paused" && <Badge>{t("common.paused")}</Badge>}
              </span>
              <PathText path={p.rootPath} max={64} />
              <span className="muted small">{t("settings.repositories.stats", { snapshots: p.snapshotCount, bytes: formatBytes(p.storedBytes) })}</span>
            </div>
            <div className="repo-actions">
              <Toggle
                checked={p.autoProtect}
                onChange={(v) => void wrap(() => api.setAutoProtect(p.id, v), v ? t("toast.autoOn") : t("toast.autoOff"))}
                label={t("settings.repositories.auto")}
              />
              {p.monitoring === "active" ? (
                <Button size="sm" variant="ghost" icon={<PauseCircle size={14} />} onClick={() => setRemoving(p)}>
                  {t("settings.repositories.stop")}
                </Button>
              ) : (
                <Button size="sm" variant="ghost" icon={<PlayCircle size={14} />} onClick={() => void wrap(() => api.setMonitoring(p.id, true), t("settings.repositories.resumed"))}>
                  {t("settings.repositories.resume")}
                </Button>
              )}
              <Button size="sm" variant="danger" icon={<Trash2 size={14} />} onClick={() => { setDeleting(p); setConfirmText(""); }}>
                {t("settings.repositories.delete")}
              </Button>
            </div>
          </li>
        ))}
      </ul>

      <Dialog
        open={!!removing}
        onClose={() => setRemoving(null)}
        title={t("settings.repositories.stopTitle", { name: removing?.name ?? "" })}
        footer={
          <>
            <Button variant="ghost" onClick={() => setRemoving(null)}>{t("common.cancel")}</Button>
            <Button
              variant="primary"
              onClick={() => {
                const p = removing!;
                setRemoving(null);
                void wrap(() => api.removeProject(p.id, false), t("settings.repositories.stopped"));
              }}
            >
              {t("settings.repositories.stopConfirm")}
            </Button>
          </>
        }
      >
        <p>{t("settings.repositories.stopBody")}</p>
      </Dialog>

      <Dialog
        open={!!deleting}
        onClose={() => setDeleting(null)}
        tone="red"
        title={t("settings.repositories.deleteTitle", { name: deleting?.name ?? "" })}
        footer={
          <>
            <Button variant="ghost" onClick={() => setDeleting(null)}>{t("common.cancel")}</Button>
            <Button
              variant="danger"
              icon={<Trash2 size={14} />}
              disabled={confirmText.trim() !== deleting?.name}
              onClick={() => {
                const p = deleting!;
                setDeleting(null);
                void wrap(() => api.removeProject(p.id, true), t("settings.repositories.deleted"));
              }}
            >
              {t("settings.repositories.deleteConfirm")}
            </Button>
          </>
        }
      >
        <p>{t("settings.repositories.deleteBody", { count: deleting?.snapshotCount ?? 0 })}</p>
        <p className="muted">{t("settings.repositories.deleteRepoUntouched")}</p>
        <Field label={t("settings.repositories.typeName", { name: deleting?.name ?? "" })}>
          <TextInput value={confirmText} onChange={(e) => setConfirmText(e.target.value)} mono autoFocus />
        </Field>
      </Dialog>
    </>
  );
}

function DiagnosticsPanel() {
  const { t } = useI18n();
  const info = useAppInfo();
  const toast = useToast();
  const env = info.data?.environment;
  const exportDiag = async () => {
    if (!isTauri()) return;
    const path = await saveDialog({
      defaultPath: `worktree-rescue-diagnostics-${new Date().toISOString().slice(0, 10)}.json`,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (!path) return;
    try {
      const written = await api.exportDiagnostics(path);
      toast({ tone: "cyan", title: t("diagnostics.exported"), body: written });
    } catch (e) {
      toast({ tone: "red", title: t("toast.actionFailed"), body: (e as Error).message });
    }
  };
  return (
    <div className="diag">
      <h3 className="block-title">{t("diagnostics.title")}</h3>
      {env && (
        <dl className="facts">
          <dt>Git</dt>
          <dd className="mono">{env.gitVersion ?? t("onboarding.env.gitMissing")}</dd>
          <dt>{t("diagnostics.gitPath")}</dt>
          <dd>{env.gitPath ? <PathText path={env.gitPath} max={60} /> : "—"}</dd>
          <dt>{t("diagnostics.os")}</dt>
          <dd className="mono">{env.os}</dd>
          <dt>{t("diagnostics.log")}</dt>
          <dd>{info.data && <PathText path={info.data.logPath} max={60} />}</dd>
        </dl>
      )}
      <p className="muted small">{t("diagnostics.desc")}</p>
      <div className="action-row">
        <Button icon={<Download size={14} />} onClick={exportDiag} disabled={!isTauri()}>
          {t("diagnostics.export")}
        </Button>
      </div>
    </div>
  );
}

function AboutPanel() {
  const { t, locale } = useI18n();
  const info = useAppInfo();
  const env = info.data?.environment;
  return (
    <div className="about">
      <p>
        <strong>Worktree Rescue {env?.appVersion ?? ""}</strong> — {t("about.tagline")}
      </p>
      <p className="muted">{t("about.license")}</p>
      <p className="muted small">{env && t("about.checked", { when: formatDateTime(env.checkedAt, locale) })}</p>
      <p className="muted small">{t("about.signing")}</p>
    </div>
  );
}
