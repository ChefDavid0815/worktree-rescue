import clsx from "clsx";
import { ArrowRight, Check, FlaskConical, Languages, ShieldCheck, X } from "lucide-react";
import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useI18n } from "../../lib/i18n";
import { api } from "../../lib/ipc";
import { keys, useSettings } from "../../lib/queries";
import { formatBytes } from "../../lib/format";
import type { AppInfo, Project, RepoProbe } from "../../lib/types";
import { Button, ErrorBlock, PathText, Segmented, Toggle, useToast } from "../../components/ui";
import { LogoMark } from "../../components/LogoMark";
import { useNav } from "../../app/nav";
import { RepoPicker } from "./RepoPicker";

type Step = "environment" | "repository" | "ready";
const STEPS: Step[] = ["environment", "repository", "ready"];

export function Onboarding({ info, onDone }: { info: AppInfo; onDone: () => void }) {
  const { t } = useI18n();
  const qc = useQueryClient();
  const nav = useNav();
  const toast = useToast();
  const settings = useSettings();
  const [step, setStep] = useState<Step>("environment");
  const [probe, setProbe] = useState<RepoProbe | null>(null);
  const [auto, setAuto] = useState(false);
  const [added, setAdded] = useState<Project | null>(null);
  const [busy, setBusy] = useState<null | "add" | "demo">(null);
  const [error, setError] = useState<unknown>(null);
  const env = info.environment;
  const envOk = env.gitFound && env.gitSupported && env.dataDirWritable;

  const finish = async () => {
    await api.completeOnboarding();
    await qc.invalidateQueries({ queryKey: keys.appInfo });
    onDone();
  };

  const add = async () => {
    if (!probe) return;
    setBusy("add");
    setError(null);
    try {
      const p = await api.addProject(probe.mainWorktree, auto);
      setAdded(p);
      nav.setProject(p.id);
      await qc.invalidateQueries({ queryKey: keys.projects });
      setStep("ready");
    } catch (e) {
      setError(e);
    } finally {
      setBusy(null);
    }
  };

  const demo = async () => {
    setBusy("demo");
    setError(null);
    try {
      const p = await api.buildDemo();
      nav.setProject(p.id);
      nav.go("overview");
      await qc.invalidateQueries({ queryKey: keys.projects });
      await finish();
      toast({ tone: "neutral", title: t("demo.ready"), body: t("demo.banner") });
    } catch (e) {
      setError(e);
    } finally {
      setBusy(null);
    }
  };

  const setLanguage = async (language: "auto" | "en" | "zh-CN") => {
    if (!settings.data) return;
    const saved = await api.updateSettings({ ...settings.data, language });
    qc.setQueryData(keys.settings, saved);
  };

  return (
    <div className="onboarding">
      <aside className="ob-side">
        <LogoMark size={52} />
        <h1 className="ob-brand">Worktree Rescue</h1>
        <p className="ob-claim">{t("onboarding.claim")}</p>
        <ol className="ob-steps">
          {STEPS.map((s, i) => (
            <li key={s} className={clsx(step === s && "is-active", STEPS.indexOf(step) > i && "is-done")}>
              <span className="ob-step-mark">{STEPS.indexOf(step) > i ? <Check size={12} /> : i + 1}</span>
              {t(`onboarding.step.${s}` as "onboarding.step.environment")}
            </li>
          ))}
        </ol>
        <div className="ob-lang">
          <Languages size={14} aria-hidden />
          <Segmented
            label={t("settings.language")}
            size="sm"
            value={settings.data?.language ?? "auto"}
            onChange={(v) => void setLanguage(v)}
            options={[
              { value: "auto", label: t("settings.language.auto") },
              { value: "en", label: "EN" },
              { value: "zh-CN", label: "中文" },
            ]}
          />
        </div>
      </aside>

      <main className="ob-main">
          <div key={step} className="ob-panel view-enter">
            {step === "environment" && (
              <>
                <h2 className="ob-title">{t("onboarding.env.title")}</h2>
                <p className="ob-lead">{t("onboarding.env.lead")}</p>
                <ul className="check-list">
                  <CheckItem ok={env.gitFound && env.gitSupported} label={t("onboarding.env.git")}>
                    {env.gitFound ? (
                      <>
                        <span className="mono">{env.gitVersion}</span>
                        {env.gitPath && <PathText path={env.gitPath} max={56} />}
                        {!env.gitSupported && <span className="tone-text-amber">{t("onboarding.env.gitOld")}</span>}
                      </>
                    ) : (
                      <span className="tone-text-red">{t("onboarding.env.gitMissing")}</span>
                    )}
                  </CheckItem>
                  <CheckItem ok={env.dataDirWritable} label={t("onboarding.env.data")}>
                    <PathText path={env.dataDir} max={64} />
                    <span className="muted">{t("onboarding.env.free", { bytes: formatBytes(env.availableBytes) })}</span>
                  </CheckItem>
                  <CheckItem ok label={t("onboarding.env.recovery")}>
                    <PathText path={env.recoveryDir} max={64} />
                  </CheckItem>
                </ul>
                <div className="ob-privacy">
                  <h3 className="block-title">{t("onboarding.privacy.title")}</h3>
                  <ul className="promise-list">
                    {(["offline", "noTelemetry", "noUpload", "noAccount", "readOnly"] as const).map((k) => (
                      <li key={k}>{t(`privacy.${k}` as "privacy.offline")}</li>
                    ))}
                  </ul>
                </div>
                <div className="ob-actions">
                  <Button variant="primary" size="lg" trailing={<ArrowRight size={16} />} onClick={() => setStep("repository")} disabled={!envOk}>
                    {t("onboarding.continue")}
                  </Button>
                  {!envOk && <p className="tone-text-amber">{t("onboarding.env.blocked")}</p>}
                </div>
              </>
            )}

            {step === "repository" && (
              <>
                <h2 className="ob-title">{t("onboarding.repo.title")}</h2>
                <p className="ob-lead">{t("onboarding.repo.desc")}</p>
                <RepoPicker probe={probe} onProbe={setProbe} />
                {probe && !probe.alreadyRegistered && (
                  <div className="ob-auto">
                    <Toggle checked={auto} onChange={setAuto} label={t("onboarding.auto.label")} description={t("onboarding.auto.desc")} />
                  </div>
                )}
                {error !== null && <ErrorBlock error={error} />}
                <div className="ob-actions">
                  <Button variant="primary" size="lg" onClick={add} loading={busy === "add"} disabled={!probe || !!probe.alreadyRegistered || busy !== null}>
                    {t("onboarding.repo.add")}
                  </Button>
                  <span className="ob-or">{t("onboarding.or")}</span>
                  <Button icon={<FlaskConical size={15} />} onClick={demo} loading={busy === "demo"} disabled={busy !== null}>
                    {t("demo.open")}
                  </Button>
                </div>
                <p className="muted small">{t("demo.explain")}</p>
              </>
            )}

            {step === "ready" && added && (
              <>
                <div className="ob-ready-mark">
                  <ShieldCheck size={26} />
                </div>
                <h2 className="ob-title">{t("onboarding.ready.title", { name: added.name })}</h2>
                <p className="ob-lead">{added.autoProtect ? t("onboarding.ready.auto") : t("onboarding.ready.manual")}</p>
                <div className="scope-table">
                  <h3 className="block-title">{t("onboarding.scope.title")}</h3>
                  <ul>
                    <li className="in"><Check size={14} />{t("onboarding.scope.tracked")}</li>
                    <li className="in"><Check size={14} />{t("onboarding.scope.staged")}</li>
                    <li className="in"><Check size={14} />{t("onboarding.scope.untracked")}</li>
                    <li className="out"><X size={14} />{t("onboarding.scope.ignored")}</li>
                    <li className="out"><X size={14} />{t("onboarding.scope.caches")}</li>
                    <li className="out"><X size={14} />{t("onboarding.scope.sensitive")}</li>
                    <li className="out"><X size={14} />{t("onboarding.scope.large", { size: formatBytes(settings.data?.maxFileBytes ?? 0, 0) })}</li>
                  </ul>
                  <p className="muted small">{t("onboarding.scope.adjust")}</p>
                </div>
                <p className="ob-limit">{t("onboarding.limit")}</p>
                <div className="ob-actions">
                  <Button variant="primary" size="lg" trailing={<ArrowRight size={16} />} onClick={finish}>
                    {t("onboarding.open")}
                  </Button>
                </div>
              </>
            )}
          </div>
      </main>
    </div>
  );
}

function CheckItem({ ok, label, children }: { ok: boolean; label: string; children: React.ReactNode }) {
  return (
    <li className={clsx("check-item", ok ? "is-ok" : "is-bad")}>
      <span className="check-mark">{ok ? <Check size={13} /> : <X size={13} />}</span>
      <span className="check-label">{label}</span>
      <span className="check-detail">{children}</span>
    </li>
  );
}
