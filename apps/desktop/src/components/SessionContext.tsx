import { AlertTriangle, CircleHelp, Clipboard, LoaderCircle, RefreshCw } from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { formatCompact } from "../lib/format";
import type { ContextBrowserCategory, Locale, SessionContext as SessionContextData } from "../types";

type SessionContextProps = {
  context?: SessionContextData;
  locale: Locale;
  isLoading: boolean;
  isError: boolean;
  onRetry: () => void;
};

const structureKeys = ["system", "tools", "user", "injected", "assistant", "tool_use", "tool_result"];

function coverageClass(value: string) {
  return value === "observed" ? "is-observed" : value === "estimated" ? "is-estimated" : "is-missing";
}

export function SessionContext({ context, locale, isLoading, isError, onRetry }: SessionContextProps) {
  const { t } = useTranslation();
  const [copyState, setCopyState] = useState<"idle" | "copied" | "failed">("idle");
  const previewText = useMemo(() => {
    if (!context) return "";
    const lines = context.browser.categories
      .flatMap((category) => category.items.map((item) => item.text).filter((value): value is string => Boolean(value)));
    return lines.join("\n\n");
  }, [context]);

  const copyPreview = async () => {
    if (!previewText) return;
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(previewText);
      } else {
        const textarea = document.createElement("textarea");
        textarea.value = previewText;
        textarea.style.position = "fixed";
        textarea.style.opacity = "0";
        document.body.appendChild(textarea);
        textarea.select();
        document.execCommand("copy");
        textarea.remove();
      }
      setCopyState("copied");
      window.setTimeout(() => setCopyState("idle"), 1_800);
    } catch {
      setCopyState("failed");
    }
  };

  if (isLoading && !context) return <div className="context-state"><LoaderCircle className="spin" size={18} /><span>{t("sessions.contextLoading")}</span></div>;
  if (isError && !context) return <div className="context-state context-state-error"><AlertTriangle size={18} /><p>{t("sessions.contextError")}</p><button className="button secondary" onClick={onRetry}><RefreshCw size={13} />{t("actions.retry")}</button></div>;
  if (!context) return <div className="context-state"><CircleHelp size={18} /><span>{t("sessions.contextNotRecorded")}</span></div>;

  const categoryLabels: Record<string, string> = {
    system: t("sessions.contextCategory.system"),
    tools: t("sessions.contextCategory.tools"),
    user: t("sessions.contextCategory.user"),
    injected: t("sessions.contextCategory.injected"),
    assistant: t("sessions.contextCategory.assistant"),
    tool_use: t("sessions.contextCategory.toolUse"),
    tool_result: t("sessions.contextCategory.toolResult"),
  };
  const categoryMetrics = new Map(context.composition.filter((metric) => structureKeys.includes(metric.key)).map((metric) => [metric.key, metric]));
  const structureMetrics = context.browser.categories.map((category) => {
    const metric = categoryMetrics.get(category.key);
    const hasItemTokens = category.items.length > 0 && category.items.every((item) => item.tokens !== undefined);
    const itemTokens = hasItemTokens ? category.items.reduce((sum, item) => sum + (item.tokens ?? 0), 0) : undefined;
    return {
      key: category.key,
      tokens: metric?.tokens ?? itemTokens,
      coverage: metric?.tokens !== undefined ? metric.coverage : category.coverage,
    };
  });
  const chartMetrics = structureMetrics.filter((metric) => metric.tokens !== undefined);
  const chartTotal = chartMetrics.reduce((sum, metric) => sum + (metric.tokens ?? 0), 0);
  const hasChartTokens = chartMetrics.length > 0;
  const totalStructureItems = context.browser.categories.reduce((sum, category) => sum + category.items.reduce((count, item) => count + item.count, 0), 0);
  const summaryCards: Array<{ key: string; value?: number; label: string; coverage?: string }> = [
    { key: "total", value: context.summary.totalTokens, label: t("sessions.contextMetric.total"), coverage: context.summary.coverage },
    { key: "input", value: context.summary.inputTokens, label: t("sessions.contextMetric.input"), coverage: context.summary.coverage },
    { key: "output", value: context.summary.outputTokens, label: t("sessions.contextMetric.output"), coverage: context.summary.coverage },
    { key: "cache", value: context.summary.cacheTokens, label: t("sessions.contextMetric.cache"), coverage: context.summary.coverage },
    { key: "compactions", value: context.summary.compactions, label: t("sessions.contextMetric.compactions"), coverage: context.summary.eventCount ? "observed" : "not-recorded" },
  ];

  return (
    <section className="session-context" aria-label={t("sessions.contextTitle")}>
      <header className="context-intro">
        <div><span className="eyebrow">{t("sessions.contextEyebrow")}</span><h3>{t("sessions.contextTitle")}</h3><p>{t("sessions.contextBody")}</p></div>
        <span className={`context-coverage ${coverageClass(context.summary.coverage)}`}>{t(`sessions.contextCoverage.${context.summary.coverage}`, { defaultValue: context.summary.coverage })}</span>
      </header>

      <div className="context-summary-grid">
        {summaryCards.map((card) => (
          <article className={`context-stat ${card.value === undefined ? "is-unknown" : ""}`} key={card.key}>
            <span>{card.label}</span>
            <strong>{card.value === undefined ? t("metrics.notRecorded") : formatCompact(card.value, locale)}</strong>
            <small className={coverageClass(card.coverage ?? "not-recorded")}>{t(`sessions.contextCoverage.${card.coverage}`, { defaultValue: card.coverage })}</small>
          </article>
        ))}
      </div>

      <section className="context-card context-structure-card">
        <header><div><h4>{t("sessions.contextComposition")}</h4><p>{t("sessions.contextCompositionBody")}</p></div><span>{hasChartTokens ? formatCompact(chartTotal, locale) : totalStructureItems ? t("sessions.contextItemCount", { count: totalStructureItems }) : t("metrics.notRecorded")}</span></header>
        {hasChartTokens && chartTotal ? <div className="context-composition-bar" aria-label={t("sessions.contextComposition")}>
          {chartMetrics.map((metric) => <i key={metric.key} className={`context-segment segment-${metric.key}`} style={{ width: `${((metric.tokens ?? 0) / chartTotal) * 100}%` }} title={`${categoryLabels[metric.key] ?? metric.key}: ${formatCompact(metric.tokens ?? 0, locale)}`} />)}
        </div> : <div className="context-empty-inline"><CircleHelp size={15} />{t("sessions.contextCompositionMissing")}</div>}
        <div className="context-composition-legend">
          {chartMetrics.map((metric) => <span key={metric.key}><i className={`context-dot segment-${metric.key}`} />{categoryLabels[metric.key] ?? metric.key}<strong>{formatCompact(metric.tokens ?? 0, locale)}</strong></span>)}
        </div>
        <div className="context-structure-grid">
          {context.browser.categories.map((category: ContextBrowserCategory) => {
            const metric = structureMetrics.find((entry) => entry.key === category.key);
            const itemCount = category.items.reduce((sum, item) => sum + item.count, 0);
            return <article className={`context-structure-item segment-${category.key}`} key={category.key}>
              <div><i className="context-dot" /><strong>{categoryLabels[category.key] ?? category.key}</strong></div>
              <b>{metric?.tokens !== undefined ? formatCompact(metric.tokens, locale) : itemCount ? t("sessions.contextItemCount", { count: itemCount }) : t("metrics.notRecorded")}</b>
              <small className={coverageClass(metric?.tokens !== undefined ? metric.coverage : category.coverage)}>{t(`sessions.contextCoverage.${metric?.tokens !== undefined ? metric.coverage : category.coverage}`, { defaultValue: metric?.coverage ?? category.coverage })}</small>
            </article>;
          })}
        </div>
      </section>

      <section className="context-card context-browser-card">
        <header><div><h4>{t("sessions.contextBrowser")}</h4><p>{t("sessions.contextBrowserBody")}</p></div><button className="button secondary context-copy-button" disabled={!previewText} onClick={() => void copyPreview()}><Clipboard size={13} />{copyState === "copied" ? t("actions.copied") : copyState === "failed" ? t("sessions.contextCopyFailed") : t("actions.copy")}</button></header>
        <div className="context-browser-categories">
          {context.browser.categories.map((category: ContextBrowserCategory) => (
            <details className="context-category" key={category.key} open={category.items.length > 0}>
              <summary><span><strong>{categoryLabels[category.key] ?? category.key}</strong><small>{t(`sessions.contextCoverage.${category.coverage}`, { defaultValue: category.coverage })}</small></span><b>{category.items.reduce((sum, item) => sum + item.count, 0)}</b></summary>
              {category.items.length ? <div className="context-category-items">{category.items.map((item) => <article key={item.id}><header><strong>{item.label}</strong><span className="context-item-count">×{item.count}</span></header>{item.text ? <p>{item.text}</p> : null}<small className={`context-coverage ${coverageClass(item.coverage)}`}>{t(`sessions.contextCoverage.${item.coverage}`, { defaultValue: item.coverage })}</small></article>)}</div> : <div className="context-category-empty">{t("sessions.contextCategoryMissing")}</div>}
            </details>
          ))}
        </div>
      </section>
    </section>
  );
}
