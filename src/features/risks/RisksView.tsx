import clsx from "clsx";
import { ShieldCheck } from "lucide-react";
import { useState } from "react";
import { useI18n } from "../../lib/i18n";
import { useRisks } from "../../lib/queries";
import { formatRelative, pathTail } from "../../lib/format";
import { severityTone } from "../../lib/protection";
import type { Project, Risk, Severity } from "../../lib/types";
import { Badge, EmptyState, ErrorBlock, Segmented, SkeletonRows } from "../../components/ui";
import { useNav } from "../../app/nav";

const ORDER: Severity[] = ["critical", "attention", "info"];

export function RisksView({ project }: { project: Project }) {
  const { t } = useI18n();
  const [scope, setScope] = useState<"open" | "all">("open");
  const risks = useRisks(project.id, scope === "all");
  const list = risks.data ?? [];
  return (
    <div className="page">
      <header className="page-header">
        <div>
          <h1 className="page-title">{t("risks.title")}</h1>
          <p className="page-sub">{t("risks.sub")}</p>
        </div>
        <Segmented
          label={t("risks.scope")}
          value={scope}
          onChange={setScope}
          options={[
            { value: "open", label: t("risks.scope.open") },
            { value: "all", label: t("risks.scope.all") },
          ]}
        />
      </header>
      {risks.error && <ErrorBlock error={risks.error} onRetry={() => risks.refetch()} />}
      {risks.isLoading && <SkeletonRows rows={4} />}
      {!risks.isLoading && list.length === 0 && (
        <EmptyState icon={<ShieldCheck size={22} />} title={t("risks.empty.title")} body={t("risks.empty.body")} />
      )}
      {ORDER.map((sev) => {
        const group = list.filter((r) => r.severity === sev && (scope === "all" || !r.resolvedAt));
        if (group.length === 0) return null;
        return (
          <section key={sev} className="section">
            <div className="section-head">
              <h2 className={clsx("section-title", `sev-title-${sev}`)}>
                {t(`severity.${sev}.group` as "severity.info.group")} <span className="count">{group.length}</span>
              </h2>
            </div>
            <ul className="risk-list">
              {group.map((r) => (
                <RiskRow key={r.id} r={r} />
              ))}
            </ul>
          </section>
        );
      })}
      <p className="footnote">{t("risks.footnote")}</p>
    </div>
  );
}

function RiskRow({ r }: { r: Risk }) {
  const { t, td, locale } = useI18n();
  const nav = useNav();
  const selected = nav.selection?.type === "risk" && nav.selection.id === r.id;
  return (
    <li>
      <button
        className={clsx("risk-row", `sev-${r.severity}`, selected && "is-selected", r.resolvedAt && "is-resolved")}
        onClick={() => nav.select({ type: "risk", id: r.id })}
      >
        <span className={clsx("risk-bar", `tone-${severityTone(r.severity)}`)} aria-hidden />
        <span className="risk-main">
          <span className="risk-title">{td(`risk.${r.rule}.title`, r.rule)}</span>
          <span className="risk-sub">
            {r.worktreePath && <span className="mono">{pathTail(r.worktreePath, 2)}</span>}
            <span>{t("risks.detected", { when: formatRelative(r.detectedAt, locale) })}</span>
            {r.resolvedAt && <span>{t("risks.resolved", { when: formatRelative(r.resolvedAt, locale) })}</span>}
          </span>
        </span>
        <Badge>{t(`confidence.${r.confidence}` as "confidence.high")}</Badge>
        {r.acknowledged && !r.resolvedAt && <Badge>{t("risks.acknowledged")}</Badge>}
      </button>
    </li>
  );
}
