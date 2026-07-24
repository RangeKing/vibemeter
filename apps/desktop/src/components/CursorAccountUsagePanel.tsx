import { useQuery } from "@tanstack/react-query";
import { CloudCog, LockKeyhole, Settings2 } from "lucide-react";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../lib/api";
import { formatCompact, formatCurrency, formatDate } from "../lib/format";
import { compactProviderUsageBuckets, summarizeProviderAccountUsage } from "../lib/providerUsage";
import type { Locale, RangeKey } from "../types";
import { AgentBadge } from "./ui";

export function CursorAccountUsagePanel({
  locale,
  range,
  compact = false,
}: {
  locale: Locale;
  range: RangeKey;
  compact?: boolean;
}) {
  const { t } = useTranslation();
  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  const enabled = settings.data?.cursorDashboardUsage === "true";
  const providers = useQuery({
    queryKey: ["providers"],
    queryFn: api.providers,
    enabled,
    refetchInterval: enabled ? 30_000 : false,
  });
  const cursor = providers.data?.find((provider) => provider.provider === "cursor");
  const accountUsage = cursor?.accountUsage ?? undefined;
  const summary = useMemo(
    () => accountUsage ? summarizeProviderAccountUsage(accountUsage, range) : undefined,
    [accountUsage, range],
  );
  const buckets = useMemo(
    () => compactProviderUsageBuckets(summary?.daily ?? [], compact ? 36 : 60),
    [compact, summary?.daily],
  );
  const maxTokens = Math.max(...buckets.map((bucket) => bucket.tokens), 1);
  const rangeLabel = t(`ranges.${range}`);

  if (settings.isLoading) return null;
  if (!enabled) {
    return (
      <section className={`cursor-account-panel disabled ${compact ? "compact" : ""}`} aria-label={t("cursorUsage.title")}>
        <div className="cursor-account-disabled-icon"><LockKeyhole size={18} /></div>
        <div>
          <span className="cursor-account-eyebrow">{t("cursorUsage.eyebrow")}</span>
          <h2>{t("cursorUsage.disabledTitle")}</h2>
          <p>{t("cursorUsage.disabledBody")}</p>
        </div>
        <button className="button secondary" onClick={() => void api.showSettings()}><Settings2 size={13} />{t("cursorUsage.openSettings")}</button>
      </section>
    );
  }

  if (providers.isLoading) {
    return (
      <section className={`cursor-account-panel loading ${compact ? "compact" : ""}`} aria-label={t("cursorUsage.title")}>
        <CloudCog size={19} />
        <span>{t("cursorUsage.loading")}</span>
      </section>
    );
  }

  if (!summary) {
    return (
      <section className={`cursor-account-panel unavailable ${compact ? "compact" : ""}`} aria-label={t("cursorUsage.title")}>
        <CloudCog size={19} />
        <div><h2>{t("cursorUsage.unavailableTitle")}</h2><p>{t("cursorUsage.unavailableBody")}</p></div>
        <button className="button secondary" onClick={() => void api.showSettings()}><Settings2 size={13} />{t("cursorUsage.checkSettings")}</button>
      </section>
    );
  }

  const period = summary.periodStart <= summary.periodEnd
    ? `${formatDate(summary.periodStart, locale, "short")} — ${formatDate(summary.periodEnd, locale, "short")}`
    : rangeLabel;
  return (
    <section className={`cursor-account-panel ${compact ? "compact" : ""}`} aria-label={t("cursorUsage.title")}>
      <header className="cursor-account-header">
        <div>
          <AgentBadge agent="cursor" compact />
          <span className="cursor-account-api">Dashboard API</span>
        </div>
        <div>
          <span className="cursor-account-scope">{t("cursorUsage.accountRange", { range: rangeLabel })}</span>
          <small>{period}</small>
        </div>
      </header>
      <div className="cursor-account-metrics">
        <div className="featured"><span>{t("cursorUsage.tokens")}</span><strong>{formatCompact(summary.totalTokens, locale)}</strong><small>{t("cursorUsage.tokenRequests", { count: summary.tokenRequestCount })}</small></div>
        <div><span>{t("cursorUsage.apiCost")}</span><strong>{summary.apiCostUsd === undefined ? t("metrics.unavailable") : formatCurrency(summary.apiCostUsd, locale)}</strong><small>{t("cursorUsage.apiCostHelp")}</small></div>
        <div><span>{t("cursorUsage.meteredCost")}</span><strong>{summary.meteredCostUsd === undefined ? t("metrics.unavailable") : formatCurrency(summary.meteredCostUsd, locale)}</strong><small>{t("cursorUsage.meteredCostHelp")}</small></div>
        <div><span>{t("cursorUsage.requests")}</span><strong>{formatCompact(summary.requestCount, locale)}</strong><small>{t("cursorUsage.models", { count: summary.models.length })}</small></div>
      </div>
      <div className="cursor-account-chart" aria-label={t("cursorUsage.chartLabel")}>
        {buckets.map((bucket) => {
          const dateLabel = bucket.startDate === bucket.endDate
            ? formatDate(bucket.startDate, locale, "short")
            : `${formatDate(bucket.startDate, locale, "short")} — ${formatDate(bucket.endDate, locale, "short")}`;
          return (
            <i key={`${bucket.startDate}-${bucket.endDate}`} title={`${dateLabel} · ${formatCompact(bucket.tokens, locale)} Token`}>
              <span style={{ height: `${Math.max(4, (bucket.tokens / maxTokens) * 100)}%` }} />
            </i>
          );
        })}
      </div>
      <footer className="cursor-account-footer">
        <p>{t("cursorUsage.scopeNote")}</p>
        <div>
          {summary.models.slice(0, 3).map((model) => <span key={model.model}>{model.model}<b>{formatCompact(model.tokens, locale)}</b></span>)}
          {!summary.models.length ? <span>{t("cursorUsage.noEvents")}</span> : null}
        </div>
      </footer>
    </section>
  );
}
