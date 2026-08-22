import { AlertTriangle, Check, CircleHelp, Clipboard, Layers3, LoaderCircle, MessageSquare, RefreshCw, Scissors, Wrench, Zap } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { formatCompact, formatDateTime } from "../lib/format";
import type { ContextBrowserCategory, ContextTimelineEvent, Locale, SessionContext as SessionContextData } from "../types";

type SessionContextProps = {
  context?: SessionContextData;
  locale: Locale;
  isLoading: boolean;
  isError: boolean;
  loadingMore: boolean;
  onRetry: () => void;
  onLoadMore: () => void;
};

const chartKeys = ["input", "cache", "output"];

function eventIcon(kind: string) {
  if (kind === "compact") return <Scissors size={13} />;
  if (kind === "tool") return <Wrench size={13} />;
  if (kind === "input") return <MessageSquare size={13} />;
  if (kind === "error") return <AlertTriangle size={13} />;
  if (kind === "model") return <Zap size={13} />;
  if (kind === "file") return <Layers3 size={13} />;
  return <CircleHelp size={13} />;
}

function coverageClass(value: string) {
  return value === "observed" ? "is-observed" : value === "estimated" ? "is-estimated" : "is-missing";
}

export function SessionContext({ context, locale, isLoading, isError, loadingMore, onRetry, onLoadMore }: SessionContextProps) {
  const { t } = useTranslation();
  const [selectedEventId, setSelectedEventId] = useState<string>();
  const [copyState, setCopyState] = useState<"idle" | "copied" | "failed">("idle");

  useEffect(() => {
    if (!context?.events.length) {
      setSelectedEventId(undefined);
      return;
    }
    setSelectedEventId((current) => current && context.events.some((event) => event.id === current) ? current : context.events[0].id);
  }, [context?.events]);

  const selectedEvent = context?.events.find((event) => event.id === selectedEventId) ?? context?.events[0];
  const previewText = useMemo(() => {
    if (!context) return "";
    const lines = [
      selectedEvent ? `${selectedEvent.name} · ${selectedEvent.phase}` : "",
      ...context.browser.categories.flatMap((category) => category.items.map((item) => item.text).filter((value): value is string => Boolean(value))),
    ].filter(Boolean);
    return lines.join("\n\n");
  }, [context, selectedEvent]);

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

  const chartMetrics = context.composition.filter((metric) => chartKeys.includes(metric.key) && metric.tokens !== undefined);
  const chartTotal = chartMetrics.reduce((sum, metric) => sum + (metric.tokens ?? 0), 0);
  const categoryLabels: Record<string, string> = {
    system: t("sessions.contextCategory.system"),
    tools: t("sessions.contextCategory.tools"),
    user: t("sessions.contextCategory.user"),
    inject: t("sessions.contextCategory.inject"),
    assistant: t("sessions.contextCategory.assistant"),
    tool: t("sessions.contextCategory.tool"),
  };
  const metricLabels: Record<string, string> = {
    input: t("sessions.contextMetric.input"),
    cache: t("sessions.contextMetric.cache"),
    output: t("sessions.contextMetric.output"),
    reasoning: t("sessions.contextMetric.reasoning"),
  };
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

      <div className="context-main-grid">
        <section className="context-card context-composition-card">
          <header><div><h4>{t("sessions.contextComposition")}</h4><p>{t("sessions.contextCompositionBody")}</p></div><span>{chartTotal ? formatCompact(chartTotal, locale) : t("metrics.notRecorded")}</span></header>
          {chartTotal ? <div className="context-composition-bar" aria-label={t("sessions.contextComposition")}>
            {chartMetrics.map((metric) => <i key={metric.key} className={`context-segment segment-${metric.key}`} style={{ width: `${((metric.tokens ?? 0) / chartTotal) * 100}%` }} title={`${metricLabels[metric.key] ?? metric.key}: ${formatCompact(metric.tokens ?? 0, locale)}`} />)}
          </div> : <div className="context-empty-inline"><CircleHelp size={15} />{t("sessions.contextCompositionMissing")}</div>}
          <div className="context-composition-legend">
            {chartMetrics.map((metric) => <span key={metric.key}><i className={`context-dot segment-${metric.key}`} />{metricLabels[metric.key] ?? metric.key}<strong>{formatCompact(metric.tokens ?? 0, locale)}</strong></span>)}
            {context.composition.filter((metric) => metric.key === "reasoning" && metric.tokens !== undefined).map((metric) => <span key={metric.key}><i className="context-dot segment-reasoning" />{metricLabels[metric.key]}<strong>{formatCompact(metric.tokens ?? 0, locale)}</strong></span>)}
          </div>
        </section>

        <section className="context-card context-browser-card">
          <header><div><h4>{t("sessions.contextBrowser")}</h4><p>{t("sessions.contextBrowserBody")}</p></div><button className="button secondary context-copy-button" disabled={!previewText} onClick={() => void copyPreview()}><Clipboard size={13} />{copyState === "copied" ? t("actions.copied") : copyState === "failed" ? t("sessions.contextCopyFailed") : t("actions.copy")}</button></header>
          {selectedEvent ? <div className="context-selected-event"><span className={`context-event-icon kind-${selectedEvent.kind}`}>{eventIcon(selectedEvent.kind)}</span><div><strong>{selectedEvent.name}</strong><small>{formatDateTime(selectedEvent.occurredAt, locale)} · {selectedEvent.phase}</small></div><span className={`context-coverage ${coverageClass(selectedEvent.coverage)}`}>{t(`sessions.contextCoverage.${selectedEvent.coverage}`, { defaultValue: selectedEvent.coverage })}</span></div> : <div className="context-empty-inline"><CircleHelp size={15} />{t("sessions.contextNoSelectedEvent")}</div>}
          <div className="context-browser-categories">
            {context.browser.categories.map((category: ContextBrowserCategory) => (
              <details className="context-category" key={category.key} open={category.items.length > 0}>
                <summary><span><strong>{categoryLabels[category.key] ?? category.key}</strong><small>{t(`sessions.contextCoverage.${category.coverage}`, { defaultValue: category.coverage })}</small></span><b>{category.items.length}</b></summary>
                {category.items.length ? <div className="context-category-items">{category.items.map((item) => <article key={item.id}><header><strong>{item.label}</strong><span className={`context-coverage ${coverageClass(item.coverage)}`}>{t(`sessions.contextCoverage.${item.coverage}`, { defaultValue: item.coverage })}</span></header>{item.text ? <p>{item.text}</p> : null}</article>)}</div> : <div className="context-category-empty">{t("sessions.contextCategoryMissing")}</div>}
              </details>
            ))}
          </div>
        </section>
      </div>

      <section className="context-card context-timeline-card">
        <header><div><h4>{t("sessions.contextTimeline")}</h4><p>{t("sessions.contextTimelineBody")}</p></div><span>{t("sessions.eventCount", { count: context.summary.eventCount })}</span></header>
        {context.events.length ? <div className="context-event-list">{context.events.map((event: ContextTimelineEvent) => <button className={`context-event-row ${event.id === selectedEventId ? "is-selected" : ""} kind-${event.kind}`} key={event.id} onClick={() => setSelectedEventId(event.id)}><span className="context-event-icon">{eventIcon(event.kind)}</span><span className="context-event-copy"><strong>{event.name}</strong><small>{t(`sessions.contextEventKind.${event.kind}`, { defaultValue: event.kind })} · {event.phase}</small></span><span className="context-event-status"><small>{event.success === false ? t("sessions.failed", { count: 1 }) : event.success === true ? t("sessions.successful", { count: 1 }) : t(`sessions.contextCoverage.${event.coverage}`, { defaultValue: event.coverage })}</small><time>{formatDateTime(event.occurredAt, locale)}</time></span></button>)}</div> : <div className="context-empty"><CircleHelp size={18} /><p>{t("sessions.contextNoEvents")}</p></div>}
        {context.hasMore ? <button className="context-load-more" disabled={loadingMore} onClick={onLoadMore}>{loadingMore ? <><LoaderCircle className="spin" size={13} />{t("actions.refreshing")}</> : t("sessions.contextLoadMore")}</button> : null}
      </section>
    </section>
  );
}
