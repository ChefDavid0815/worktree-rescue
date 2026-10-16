import clsx from "clsx";
import { FolderSearch, GitBranch } from "lucide-react";
import { useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useI18n } from "../../lib/i18n";
import { api, isTauri } from "../../lib/ipc";
import { shortHash } from "../../lib/format";
import type { RepoProbe } from "../../lib/types";
import { Badge, Button, ErrorBlock, PathText, TextInput } from "../../components/ui";

/** Choose a folder, probe it read-only, and show what Worktree Rescue found. */
export function RepoPicker({ probe, onProbe }: { probe: RepoProbe | null; onProbe: (p: RepoProbe | null) => void }) {
  const { t, td } = useI18n();
  const [path, setPath] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);

  const inspect = async (p: string) => {
    if (!p.trim()) return;
    setBusy(true);
    setError(null);
    onProbe(null);
    try {
      onProbe(await api.probeRepository(p.trim()));
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  };

  const browse = async () => {
    const dir = await openDialog({ directory: true, multiple: false, title: t("onboarding.repo.browse") });
    if (typeof dir === "string") {
      setPath(dir);
      void inspect(dir);
    }
  };

  const id = probe?.identity;
  return (
    <div className="repo-picker">
      <div className="input-with-btn">
        <TextInput
          value={path}
          onChange={(e) => setPath(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && inspect(path)}
          placeholder={t("onboarding.repo.placeholder")}
          aria-label={t("onboarding.repo.placeholder")}
          mono
        />
        {isTauri() && (
          <Button icon={<FolderSearch size={15} />} onClick={browse}>
            {t("onboarding.repo.browse")}
          </Button>
        )}
        <Button onClick={() => inspect(path)} loading={busy} disabled={!path.trim()}>
          {t("onboarding.repo.inspect")}
        </Button>
      </div>
      {error !== null && <ErrorBlock error={error} />}
      {probe && id && (
        <div className={clsx("probe", probe.alreadyRegistered && "is-registered")}>
          <div className="probe-head">
            <GitBranch size={16} />
            <span className="mono probe-branch">
              {id.branch ?? (id.detached ? t("common.detached") : "—")}
            </span>
            {id.unborn && <Badge tone="amber">{t("onboarding.repo.unborn")}</Badge>}
            {id.isLinkedWorktree && <Badge>{t("onboarding.repo.linked")}</Badge>}
            {probe.alreadyRegistered && <Badge tone="amber">{t("onboarding.repo.already")}</Badge>}
          </div>
          <dl className="facts">
            <dt>{t("onboarding.repo.main")}</dt>
            <dd><PathText path={probe.mainWorktree} max={60} /></dd>
            <dt>{t("facts.head")}</dt>
            <dd className="mono">{id.headOid ? shortHash(id.headOid, 12) : "—"}</dd>
            <dt>{t("onboarding.repo.worktrees")}</dt>
            <dd className="num">{probe.worktreeCount}</dd>
            {probe.status && (
              <>
                <dt>{t("facts.changes")}</dt>
                <dd className="num">
                  {t("worktree.countsLine", {
                    staged: probe.status.staged,
                    unstaged: probe.status.unstaged,
                    untracked: probe.status.untracked,
                    deleted: probe.status.deleted,
                  })}
                </dd>
              </>
            )}
          </dl>
          <ul className="support-list">
            {probe.support.map((s) => (
              <li key={s.feature} className={`lvl-${s.level}`}>
                <Badge tone={s.level === "supported" ? "cyan" : s.level === "partial" ? "amber" : "red"}>
                  {t(`support.${s.level}` as "support.supported")}
                </Badge>
                <span>{td(`feature.${s.feature}`, s.feature)}</span>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
