import { useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useI18n } from "../../lib/i18n";
import { api } from "../../lib/ipc";
import { keys, useSettings } from "../../lib/queries";
import type { RepoProbe } from "../../lib/types";
import { Button, Dialog, ErrorBlock, Toggle, useToast } from "../../components/ui";
import { useNav } from "../../app/nav";
import { RepoPicker } from "./RepoPicker";

export function AddRepositoryDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { t } = useI18n();
  const qc = useQueryClient();
  const nav = useNav();
  const toast = useToast();
  const settings = useSettings();
  const [probe, setProbe] = useState<RepoProbe | null>(null);
  const [auto, setAuto] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);

  useEffect(() => {
    if (open) {
      setProbe(null);
      setError(null);
      setAuto(settings.data?.autoProtectNewProjects ?? false);
    }
  }, [open, settings.data]);

  const add = async () => {
    if (!probe) return;
    setBusy(true);
    setError(null);
    try {
      const p = await api.addProject(probe.mainWorktree, auto);
      await qc.invalidateQueries({ queryKey: keys.projects });
      nav.setProject(p.id);
      nav.go("overview");
      toast({ tone: "cyan", title: t("onboarding.added", { name: p.name }), body: auto ? t("toast.autoOn") : t("onboarding.addedManual") });
      onClose();
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title={t("switcher.add")}
      width={640}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button variant="primary" onClick={add} loading={busy} disabled={!probe || !!probe.alreadyRegistered}>
            {t("onboarding.repo.add")}
          </Button>
        </>
      }
    >
      <p>{t("onboarding.repo.desc")}</p>
      <RepoPicker probe={probe} onProbe={setProbe} />
      {probe && !probe.alreadyRegistered && (
        <Toggle checked={auto} onChange={setAuto} label={t("onboarding.auto.label")} description={t("onboarding.auto.desc")} />
      )}
      {error !== null && <ErrorBlock error={error} />}
    </Dialog>
  );
}
