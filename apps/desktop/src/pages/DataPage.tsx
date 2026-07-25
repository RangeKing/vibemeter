import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { EChartsCoreOption } from "echarts";
import { ArrowRight, BarChart3, CalendarDays, CheckCircle2, Merge, Sparkles, Workflow } from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { EChart } from "../components/EChart";
import { BehaviorStreams } from "../components/BehaviorStreams";
import { CursorAccountUsagePanel } from "../components/CursorAccountUsagePanel";
import { CatchphraseClouds } from "../components/CatchphraseClouds";
import { RangePicker } from "../components/RangePicker";
import { WorkEventCard } from "../components/WorkEventCard";
import { AgentBadge, EmptyState, ErrorState, LoadingState } from "../components/ui";
import { api } from "../lib/api";
import { buildHourlyActivity } from "../lib/activity";
import { chartTooltip, useChartColors } from "../lib/chartTheme";
import { agentName, formatCompact, formatCurrency, formatDate, formatDuration, tokenTotal } from "../lib/format";
import { savitzkyGolaySmooth } from "../lib/smoothing";
import { useUiStore } from "../store";
import type { DailyUsagePoint, HourlyUsagePoint, Locale, RangeKey, TaskSummary } from "../types";

type DailyTotal = { date: string; tokens: number; activeSeconds: number; sessions: number };

const RANGE_DAY_COUNTS: Partial<Record<RangeKey, number>> = {
  today: 1,
  "7d": 7,
  "30d": 30,
  "90d": 90,
  "180d": 180,
  year: 365,
};

function localDateKey(date: Date): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function groupDaily(points: DailyUsagePoint[], range: RangeKey, referenceTime: Date): DailyTotal[] {
  const grouped = new Map<string, DailyTotal>();
  for (const point of points) {
    const current = grouped.get(point.date) ?? { date: point.date, tokens: 0, activeSeconds: 0, sessions: 0 };
    current.tokens += tokenTotal(point.usage);
    current.activeSeconds += point.activeSeconds;
    current.sessions += point.sessionCount;
    grouped.set(point.date, current);
  }
  const count = RANGE_DAY_COUNTS[range];
  if (!count) return [...grouped.values()].sort((a, b) => a.date.localeCompare(b.date));
  const cursor = new Date(referenceTime);
  cursor.setHours(0, 0, 0, 0);
  cursor.setDate(cursor.getDate() - count + 1);
  for (let index = 0; index < count; index += 1) {
    const date = localDateKey(cursor);
    if (!grouped.has(date)) grouped.set(date, { date, tokens: 0, activeSeconds: 0, sessions: 0 });
    cursor.setDate(cursor.getDate() + 1);
  }
  return [...grouped.values()].sort((a, b) => a.date.localeCompare(b.date));
}

type AgentTrendSeries = { agent: string; buckets: string[]; values: number[]; total: number };

function agentSeries(daily: DailyUsagePoint[], hourly: HourlyUsagePoint[], useHourly: boolean, referenceTime: Date, knownAgents: string[] = []): AgentTrendSeries[] {
  const hourBuckets = useHourly ? buildHourlyActivity(hourly, "today", referenceTime).map((item) => item.key) : [];
  const buckets = useHourly ? hourBuckets : [...new Set(daily.map((point) => point.date))].sort();
  const points = useHourly
    ? hourly.map((point) => ({ bucket: point.hour, agent: point.agent, tokens: tokenTotal(point.usage) }))
    : daily.map((point) => ({ bucket: point.date, agent: point.agent, tokens: tokenTotal(point.usage) }));
  const agents = [...new Set([...knownAgents, ...points.map((point) => point.agent)])];
  return agents.map((agent) => ({
    agent,
    buckets,
    values: buckets.map((bucket) => points.filter((point) => point.agent === agent && point.bucket === bucket).reduce((sum, point) => sum + point.tokens, 0)),
    total: points.filter((point) => point.agent === agent).reduce((sum, point) => sum + point.tokens, 0),
  }));
}

function agentColor(agent: string, colors: ReturnType<typeof useChartColors>, fallbackIndex = 0): string {
  if (agent === "claude-code" || agent === "claude") return colors.agent[1];
  if (agent === "codex") return colors.agent[0];
  if (agent === "kimi-code" || agent === "kimi") return colors.agent[2];
  if (agent === "cursor") return colors.agent[3];
  if (agent === "openclaw") return colors.agent[4];
  if (agent === "hermes") return colors.agent[5];
  return colors.agent[(fallbackIndex + 3) % colors.agent.length];
}

function trendOption(series: AgentTrendSeries[], colors: ReturnType<typeof useChartColors>, locale: Locale, hourly: boolean): EChartsCoreOption {
  return {
    grid: { left: 10, right: 18, top: 12, bottom: 26, containLabel: true },
    tooltip: { ...chartTooltip(colors), trigger: "axis", valueFormatter: (value: unknown) => formatCompact(Number(value), locale) },
    xAxis: { type: "category", boundaryGap: false, data: series[0]?.buckets ?? [], axisLine: { lineStyle: { color: colors.hairline } }, axisTick: { show: false }, axisLabel: { color: colors.textTertiary, fontSize: 9, hideOverlap: true, formatter: (value: string) => hourly ? value.slice(11, 16) : value.slice(5) } },
    yAxis: { type: "value", splitNumber: 3, axisLabel: { color: colors.textTertiary, fontSize: 9, formatter: (value: number) => formatCompact(value, locale) }, splitLine: { lineStyle: { color: colors.hairline } } },
    series: series.map((item, index) => ({
      name: agentName(item.agent),
      type: "line",
      data: savitzkyGolaySmooth(item.values),
      showSymbol: false,
      smooth: .12,
      smoothMonotone: "x",
      tooltip: { valueFormatter: (_value: unknown, dataIndex: number) => formatCompact(item.values[dataIndex] ?? 0, locale) },
      lineStyle: { width: 2.6, color: agentColor(item.agent, colors, index) },
      itemStyle: { color: agentColor(item.agent, colors, index) },
      areaStyle: { color: agentColor(item.agent, colors, index), opacity: .07 },
      emphasis: { focus: "series" },
    })),
  };
}

function distributionOption(agents: Array<{ label: string; value: number }>, models: Array<{ label: string; value: number }>, colors: ReturnType<typeof useChartColors>, locale: Locale, labels: { agent: string; model: string }): EChartsCoreOption {
  return {
    tooltip: { ...chartTooltip(colors), trigger: "item", formatter: (item: { seriesName: string; name: string; value: number; percent: number }) => `${item.seriesName}<br/>${item.name}<br/>${formatCompact(item.value, locale)} · ${Math.round(item.percent)}%` },
    legend: { show: false },
    series: [
      { name: labels.agent, type: "pie", radius: ["60%", "82%"], center: ["50%", "50%"], padAngle: 2, itemStyle: { borderRadius: 5, borderColor: colors.paper, borderWidth: 2 }, label: { show: false }, data: agents.map((item, index) => ({ ...item, itemStyle: { color: agentColor(item.label, colors, index) } })) },
      { name: labels.model, type: "pie", radius: ["34%", "52%"], center: ["50%", "50%"], padAngle: 1, itemStyle: { borderRadius: 3, borderColor: colors.paper, borderWidth: 1 }, label: { show: false }, data: models.slice(0, 8).map((item, index) => ({ ...item, itemStyle: { color: colors.model[index % colors.model.length] } })) },
    ],
  };
}

function toolOption(tools: Array<{ label: string; value: number }>, colors: ReturnType<typeof useChartColors>, locale: Locale): EChartsCoreOption {
  const visible = tools.slice(0, 7).reverse();
  return {
    grid: { left: 8, right: 16, top: 5, bottom: 4, containLabel: true },
    tooltip: { ...chartTooltip(colors), trigger: "axis", axisPointer: { type: "shadow" }, valueFormatter: (value: unknown) => formatCompact(Number(value), locale) },
    xAxis: { type: "value", show: false },
    yAxis: { type: "category", data: visible.map((item) => item.label), axisLine: { show: false }, axisTick: { show: false }, axisLabel: { color: colors.textSecondary, fontSize: 10, width: 100, overflow: "truncate" } },
    series: [{ type: "bar", data: visible.map((item, index) => ({ value: item.value, itemStyle: { color: colors.series[index % colors.series.length], borderRadius: [0, 5, 5, 0] } })), barWidth: 10, label: { show: true, position: "right", color: colors.textTertiary, fontSize: 9, formatter: (item: { value: number }) => formatCompact(item.value, locale) } }],
  };
}

function openTask(task: TaskSummary) {
  if (task.primarySessionId) useUiStore.getState().selectSession(task.primarySessionId);
  useUiStore.getState().setPage("sessions");
}

export function DataPage({ locale }: { locale: Locale }) {
  const { t } = useTranslation();
  const colors = useChartColors();
  const client = useQueryClient();
  const range = useUiStore((state) => state.range);
  const setPage = useUiStore((state) => state.setPage);
  const [selectedTasks, setSelectedTasks] = useState<string[]>([]);
  const [showAllEvents, setShowAllEvents] = useState(false);
  const overview = useQuery({ queryKey: ["overview", range], queryFn: () => api.overview(range), refetchInterval: 30_000 });
  const phrases = useQuery({ queryKey: ["phrase-cloud", range], queryFn: () => api.phraseCloud(range), refetchInterval: 30_000 });
  const sources = useQuery({ queryKey: ["sources"], queryFn: api.sources, refetchInterval: 30_000 });
  const tasks = useQuery({ queryKey: ["tasks", range], queryFn: () => api.tasks(range), refetchInterval: 30_000 });
  const merge = useMutation({
    mutationFn: (taskIds: string[]) => api.mergeTasks(taskIds),
    onSuccess: async () => {
      setSelectedTasks([]);
      await Promise.all([client.invalidateQueries({ queryKey: ["tasks"] }), client.invalidateQueries({ queryKey: ["overview"] }), client.invalidateQueries({ queryKey: ["sessions"] })]);
    },
  });
  const isToday = range === "today";
  const referenceTime = useMemo(() => {
    const value = new Date(overview.data?.generatedAt ?? Date.now());
    return Number.isNaN(value.getTime()) ? new Date() : value;
  }, [overview.data?.generatedAt]);
  const sourceAgents = useMemo(() => (sources.data ?? []).map((source) => source.agent), [sources.data]);
  const series = useMemo(() => agentSeries(overview.data?.daily ?? [], overview.data?.hourly ?? [], isToday, referenceTime, sourceAgents), [overview.data?.daily, overview.data?.hourly, isToday, referenceTime, sourceAgents]);
  const daily = useMemo(() => groupDaily(overview.data?.daily ?? [], range, referenceTime), [overview.data?.daily, range, referenceTime]);
  const hourlyActivity = useMemo(() => isToday ? buildHourlyActivity(overview.data?.hourly ?? [], "today", referenceTime) : [], [isToday, overview.data?.hourly, referenceTime]);
  const maxActivity = Math.max(...(isToday ? hourlyActivity.map((item) => item.total) : daily.map((item) => item.tokens)), 1);

  if (overview.isLoading || tasks.isLoading) return <LoadingState />;
  if (overview.isError || !overview.data || tasks.isError || !tasks.data) return <ErrorState retry={() => void Promise.all([overview.refetch(), tasks.refetch()])} />;
  const data = overview.data;
  const displayedAgents = sourceAgents.map((agent) => data.agents.find((item) => item.label === agent) ?? { id: agent, label: agent, value: 0 });
  const visibleTasks = showAllEvents ? tasks.data : tasks.data.slice(0, 12);
  const reviewed = tasks.data.filter((task) => task.worthReviewing).length;
  const toggleTask = (id: string, selected: boolean) => setSelectedTasks((current) => selected ? [...new Set([...current, id])] : current.filter((item) => item !== id));
  const rangeText = t(`ranges.${range}`);
  const agentTokenText = (agent: string, value: number) => agent === "cursor"
    ? t("metrics.notRecorded")
    : formatCompact(value, locale);
  const activeHours = hourlyActivity.filter((item) => item.total > 0);
  const hourWindow = activeHours.length ? `${activeHours[0].startAt.slice(11, 16)} — ${activeHours[activeHours.length - 1].startAt.slice(11, 16)}` : t("data.noHourlyActivity");

  return (
    <div className="page data-page">
      <header className="data-header">
        <div><span className="eyebrow"><BarChart3 size={13} />{t("data.eyebrow")}</span><h1>{t("data.title")}</h1><p>{t("data.description")}</p></div>
        <RangePicker />
      </header>

      <section className="data-ledger" aria-label={t("data.summary")}>
        <div><span>{t("metrics.sessions")}</span><strong>{formatCompact(data.totals.sessionCount, locale)}</strong><small>{rangeText}</small></div>
        <div><span>{t("metrics.duration")}</span><strong>{formatDuration(data.totals.activeSeconds, locale)}</strong><small>{data.totals.activeDays} {t("metrics.activeDays").toLowerCase()}</small></div>
        <div className="featured"><span>{t("metrics.tokens")}</span><strong>{formatCompact(tokenTotal(data.totals.usage), locale)}</strong><small>{t("data.localTokenFootnote")}</small></div>
        <div className="cost"><span>{t("metrics.cost")}</span><strong>{data.totals.estimatedCostUsd !== undefined ? formatCurrency(data.totals.estimatedCostUsd, locale) : t("metrics.unavailable")}</strong><small>{t("data.observedLocally")}</small></div>
        <div><span>{t("metrics.activeDays")}</span><strong>{formatCompact(data.totals.activeDays, locale)}</strong><small>{t("data.observedLocally")}</small></div>
      </section>

      <section className="data-source-strip" aria-label={t("data.sourcesTitle")}>
        <header><span>{t("data.sourcesTitle")}</span><small>{t("data.sourcesBody")}</small></header>
        <div>{(sources.data ?? []).map((source) => <article key={source.agent} className={source.available ? "available" : "missing"}><AgentBadge agent={source.agent} compact /><span>{source.available ? `${formatCompact(source.sessionCount, locale)} ${t("metrics.sessions")}` : t("data.sourceMissing")}</span></article>)}</div>
      </section>

      <CursorAccountUsagePanel locale={locale} range={range} />

      {phrases.data ? <CatchphraseClouds data={phrases.data} locale={locale} /> : phrases.isError ? (
        <section className="catchphrase-section">
          <ErrorState retry={() => void phrases.refetch()} />
        </section>
      ) : null}

      <section className="data-panel trend-panel">
        <header className="panel-heading"><div><span className="section-index">01</span><h2>{t("data.trend")}</h2><p>{t(isToday ? "data.trendHourlyBody" : "data.trendBody", { range: rangeText })}</p></div><span className="panel-kicker"><Sparkles size={13} />{t("data.combinedAgents")}</span></header>
        <div className="combined-trend">
          {series.length ? <>
            <div className="trend-series-legend">{series.map((item, index) => <div key={item.agent}><i style={{ background: agentColor(item.agent, colors, index) }} /><strong>{agentName(item.agent)}</strong><small>{agentTokenText(item.agent, item.total)}</small></div>)}</div>
            <EChart option={trendOption(series, colors, locale, isToday)} ariaLabel={t("data.trend")} style={{ height: 260 }} />
          </> : <EmptyState title={t("data.noUsage")} body={t("data.noUsageBody")} />}
        </div>
      </section>

      <div className="data-analysis-grid">
        <section className="data-panel activity-panel">
          <header className="panel-heading"><div><span className="section-index">02</span><h2>{t("data.activity")}</h2><p>{t(isToday ? "data.activityHourlyBody" : "data.activityBody")}</p></div><CalendarDays size={17} /></header>
          <div className="activity-summary"><strong>{isToday ? activeHours.length : data.totals.activeDays}</strong><span>{isToday ? t("data.activeHours") : t("metrics.activeDays")}</span><small>{isToday ? hourWindow : daily.length ? `${formatDate(daily[0].date, locale, "short")} — ${formatDate(daily[daily.length - 1].date, locale, "short")}` : rangeText}</small></div>
          {isToday ? <>
            <div className="activity-heatmap hourly" style={{ gridTemplateColumns: `repeat(${Math.max(1, hourlyActivity.length)}, minmax(7px, 1fr))` }}>
              {hourlyActivity.map((item) => <span key={item.key} tabIndex={0} style={{ opacity: .12 + .88 * Math.sqrt(item.total / maxActivity) }} data-tooltip={`${item.startAt.slice(11, 16)} · ${formatCompact(item.total, locale)} Token`} aria-label={`${item.startAt.slice(11, 16)} ${formatCompact(item.total, locale)} Token`} />)}
            </div>
            {hourlyActivity.length ? <div className="hour-axis"><span>{hourlyActivity[0].startAt.slice(11, 16)}</span><span>{hourlyActivity[Math.floor(hourlyActivity.length / 2)].startAt.slice(11, 16)}</span><span>{hourlyActivity[hourlyActivity.length - 1].startAt.slice(11, 16)}</span></div> : null}
          </> : <div className="activity-heatmap" style={{ gridTemplateColumns: `repeat(${Math.max(1, Math.ceil(daily.length / 7))}, minmax(7px, 1fr))` }}>
            {daily.map((item) => <span key={item.date} tabIndex={0} style={{ opacity: .15 + .85 * Math.sqrt(item.tokens / maxActivity) }} data-tooltip={`${formatDate(item.date, locale)} · ${formatCompact(item.tokens, locale)} Token · ${item.sessions} ${t("metrics.sessions")}`} aria-label={`${formatDate(item.date, locale)} ${formatCompact(item.tokens, locale)} Token`} />)}
          </div>}
        </section>
        <section className="data-panel distribution-panel">
          <header className="panel-heading"><div><span className="section-index">03</span><h2>{t("data.distribution")}</h2><p>{t("data.distributionBody")}</p></div></header>
          <div className="distribution-layout">
            <EChart option={distributionOption(displayedAgents, data.models, colors, locale, { agent: t("data.agentLegend"), model: t("data.modelLegend") })} ariaLabel={t("data.distribution")} style={{ height: 250 }} />
            <aside className="distribution-legend">
              <div><strong>{t("data.agentLegend")}</strong>{displayedAgents.map((item, index) => <span key={item.id}><i style={{ background: agentColor(item.label, colors, index) }} /><b>{agentName(item.label)}</b><small>{agentTokenText(item.label, item.value)}</small></span>)}</div>
              <div><strong>{t("data.modelLegend")}</strong>{data.models.slice(0, 6).map((item, index) => <span key={item.id}><i style={{ background: colors.model[index % colors.model.length] }} /><b title={item.label}>{item.label}</b><small>{formatCompact(item.value, locale)}</small></span>)}</div>
            </aside>
          </div>
        </section>
        <section className="data-panel tools-panel">
          <header className="panel-heading"><div><span className="section-index">04</span><h2>{t("data.tools")}</h2><p>{t("data.toolsBody")}</p></div><Workflow size={17} /></header>
          <EChart option={toolOption(data.tools, colors, locale)} ariaLabel={t("data.tools")} style={{ height: 240 }} />
        </section>
      </div>

      <section className="data-panel behavior-panel">
        <header className="panel-heading"><div><span className="section-index">05</span><h2>{t("behavior.title")}</h2><p>{t("behavior.dataBody", { range: rangeText })}</p></div><Workflow size={17} /></header>
        <BehaviorStreams data={data.behavior} locale={locale} />
      </section>

      <section className="events-section">
        <header className="section-heading events-heading"><div><span className="section-index">06</span><h2>{t("data.events")}</h2><p>{t("data.eventsBody", { range: rangeText })}</p></div><div className="event-summary"><span><strong>{tasks.data.length}</strong>{t("metrics.tasks")}</span><span className={reviewed ? "attention" : "clear"}>{reviewed ? t("data.reviewCount", { count: reviewed }) : <><CheckCircle2 size={13} />{t("today.noReview")}</>}</span></div></header>
        {selectedTasks.length ? <div className="task-merge-bar"><span>{t("task.selected", { count: selectedTasks.length })}</span><button className="button secondary" onClick={() => merge.mutate(selectedTasks)} disabled={selectedTasks.length < 2 || merge.isPending}><Merge size={13} />{t("task.merge")}</button></div> : null}
        {visibleTasks.length ? <div className="event-grid">{visibleTasks.map((task) => <WorkEventCard key={task.id} task={task} locale={locale} selected={selectedTasks.includes(task.id)} onSelect={(value) => toggleTask(task.id, value)} onOpen={() => openTask(task)} onAcceptSuggestion={task.suggestedTaskId ? () => merge.mutate([task.suggestedTaskId!, task.id]) : undefined} />)}</div> : <EmptyState title={t("data.noEvents")} body={t("data.noEventsBody")} />}
        <footer className="events-footer">
          {tasks.data.length > 12 ? <button className="button subtle" onClick={() => setShowAllEvents((value) => !value)}>{showAllEvents ? t("data.showLess") : t("data.showAll", { count: tasks.data.length })}</button> : <span />}
          <button className="inline-link" onClick={() => setPage("sessions")}>{t("sessions.title")}<ArrowRight size={14} /></button>
        </footer>
      </section>
    </div>
  );
}
