import { useQuery } from "@tanstack/react-query";
import { ArrowUpRight, CircleDot, Clock3, FileCode2, Lightbulb, Scale } from "lucide-react";
import { useTranslation } from "react-i18next";
import { RangePicker } from "../components/RangePicker";
import { BehaviorStreams } from "../components/BehaviorStreams";
import { CursorAccountUsagePanel } from "../components/CursorAccountUsagePanel";
import { EmptyState, ErrorState, LoadingState, PageHeader } from "../components/ui";
import { api } from "../lib/api";
import { agentName, formatCompact, formatDuration, formatPercent, tokenTotal } from "../lib/format";
import { useUiStore } from "../store";
import type { InsightStat, Locale } from "../types";

function splitEvidenceLabel(label: string): [string, string] {
  const divider = label.lastIndexOf(" · ");
  return divider < 0 ? [label, ""] : [label.slice(0, divider), label.slice(divider + 3)];
}

function peakHourLabel(label: string): { window: string; count: number } | undefined {
  const [time, countText] = splitEvidenceLabel(label);
  const hour = Number.parseInt(time.split(":")[0] ?? "", 10);
  const count = Number.parseInt(countText, 10);
  if (!Number.isFinite(hour) || !Number.isFinite(count)) return undefined;
  const nextHour = (hour + 1) % 24;
  return { window: `${String(hour).padStart(2, "0")}:00–${String(nextHour).padStart(2, "0")}:00`, count };
}

function formatStat(stat: InsightStat, locale: Locale): string {
  if (stat.format === "text") return stat.textValue ? (stat.id === "top-agent" ? agentName(stat.textValue) : stat.textValue) : "—";
  if (stat.format === "percent") return formatPercent(stat.value, locale);
  if (stat.format === "duration") return formatDuration(Math.round(stat.value), locale);
  return formatCompact(stat.value, locale);
}

export function InsightsPage({ locale }: { locale: Locale }) {
  const { t } = useTranslation();
  const range = useUiStore((state) => state.range);
  const setPage = useUiStore((state) => state.setPage);
  const selectSession = useUiStore((state) => state.selectSession);
  const query = useQuery({ queryKey: ["insights", range], queryFn: () => api.insights(range) });
  if (query.isLoading) return <LoadingState />;
  if (query.isError || !query.data) return <ErrorState retry={() => void query.refetch()} />;
  const data = query.data;
  const maxTokens = Math.max(...data.comparison.map((item) => tokenTotal(item.usage)), 1);
  const sampleSize = data.sampleSize;
  const lowSample = sampleSize < data.minimumSampleSize;

  return (
    <div className="page insights-page">
      <PageHeader title={t("insights.title")} description={t("insights.description")} actions={<RangePicker />} />
      <section className={`insight-baseline ${lowSample ? "low-sample" : ""}`}>
        <div><CircleDot size={16} /><strong>{t("insights.sample", { count: sampleSize })}</strong></div>
        <p>{lowSample ? t("insights.insufficient.detail") : t("insights.minimum", { count: data.minimumSampleSize })}</p>
        <div className="baseline-track"><span style={{ width: `${Math.min(100, (sampleSize / data.minimumSampleSize) * 100)}%` }} /></div>
      </section>

      <CursorAccountUsagePanel locale={locale} range={range} compact />

      {data.stats.length ? (
        <section className="insight-bento">
          <header className="section-heading"><div><span className="section-index">01</span><h2>{t("insights.stats")}</h2></div></header>
          <div className="insight-bento-grid">
            {data.stats.map((stat, index) => (
              <article className={`insight-stat-tile ${index === 0 ? "hero" : ""}`} key={stat.id}>
                <span>{t(stat.labelKey)}</span>
                <strong>{formatStat(stat, locale)}</strong>
              </article>
            ))}
          </div>
        </section>
      ) : null}

      <section className="insight-behavior">
        <header className="section-heading"><div><span className="section-index">02</span><h2>{t("behavior.insightTitle")}</h2><p>{t("behavior.insightBody")}</p></div></header>
        <BehaviorStreams data={data.behavior} locale={locale} compact />
      </section>

      {data.items.length ? (
        <section className="insight-signals">
          <header className="section-heading"><div><span className="section-index">03</span><h2>{t("insights.title")}</h2></div></header>
          <div className="insight-grid">
            {data.items.map((item, index) => {
              const title = t(item.titleKey);
              const detail = t(item.detailKey);
              const fileEvidence = item.id === "high-churn-file" ? item.evidence.find((evidence) => evidence.kind === "file") : undefined;
              const peakEvidence = item.id === "peak-hour" ? item.evidence.find((evidence) => evidence.kind === "sessions") : undefined;
              const [filePath, editCount] = fileEvidence ? splitEvidenceLabel(fileEvidence.label) : ["", ""];
              const peak = peakEvidence ? peakHourLabel(peakEvidence.label) : undefined;
              const remainingEvidence = item.evidence.filter((evidence) => evidence !== fileEvidence && evidence !== peakEvidence);
              return (
                <article className={`insight-card tier-${item.tier}`} key={item.id}>
                  <header>
                    <span className="insight-number">{String(index + 1).padStart(2, "0")}</span>
                    <span className="finding-tier">{t(`insights.${item.tier}`)}</span>
                  </header>
                  <Lightbulb size={21} />
                  <h2>{title}</h2>
                  <p>{detail}</p>
                  {fileEvidence ? (
                    <button
                      className="insight-file-link"
                      disabled={!item.targetSessionId}
                      onClick={() => {
                        if (!item.targetSessionId) return;
                        selectSession(item.targetSessionId);
                        setPage("sessions");
                      }}
                    >
                      <FileCode2 size={23} />
                      <span>
                        <strong>{filePath}</strong>
                        <small>{t("insights.fileActivity", { edits: Number.parseInt(editCount, 10) || 0, sessions: item.value ?? 0 })}</small>
                      </span>
                      <span className="insight-file-action">{t("insights.openSession")}<ArrowUpRight size={15} /></span>
                    </button>
                  ) : null}
                  {peak ? (
                    <div className="insight-peak-window">
                      <Clock3 size={24} />
                      <span><strong>{peak.window}</strong><small>{t("insights.peakHourSessions", { count: peak.count })}</small></span>
                    </div>
                  ) : null}
                  {item.value != null && !fileEvidence ? (
                    <strong className="insight-value">
                      {item.value < 1 && item.id.includes("gap") ? formatPercent(item.value, locale) : formatCompact(item.value, locale)}
                    </strong>
                  ) : null}
                  {remainingEvidence.length ? <div className="evidence-chips">{remainingEvidence.map((evidence) => <span key={evidence.id}>{evidence.kind} · {evidence.label}</span>)}</div> : null}
                </article>
              );
            })}
          </div>
        </section>
      ) : (
        <EmptyState title={t("insights.noItems")} body={t("insights.description")} />
      )}

      <section className="comparison-ledger">
        <header className="section-heading"><div><span className="section-index">04</span><h2>{t("insights.comparison")}</h2></div><Scale size={18} /></header>
        <div className="comparison-list">
          {data.comparison.slice(0, 8).map((item) => (
            <div key={item.id}>
              <span><strong>{item.label}</strong><small>{item.groupKind}</small></span>
              <div className="comparison-bar"><i style={{ width: `${(tokenTotal(item.usage) / maxTokens) * 100}%` }} /></div>
              <span><strong>{formatCompact(tokenTotal(item.usage), locale)}</strong><small>{t("metrics.tokens")}</small></span>
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}
