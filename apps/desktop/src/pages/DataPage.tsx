import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { EChartsCoreOption } from "echarts";
import { ArrowRight, BarChart3, Blocks, CalendarDays, Merge, Scale, Sparkles, Workflow } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import { CursorAccountUsagePanel } from "../components/CursorAccountUsagePanel";
import { EChart } from "../components/EChart";
import { focusHeatmapIndex, HeatmapCell } from "../components/HeatmapCell";
import { RangePicker } from "../components/RangePicker";
import { SessionsWorkspace } from "../components/SessionsWorkspace";
import { SourceCapabilityLegend } from "../components/SourceCapabilityLegend";
import { WorkEventCard } from "../components/WorkEventCard";
import { AgentBadge, EmptyState, ErrorState, LoadingState } from "../components/ui";
import { api } from "../lib/api";
import { buildHourlyActivity, findPeakActivity } from "../lib/activity";
import { chartTooltip, useChartColors } from "../lib/chartTheme";
import { agentName, cacheTokenTotal, formatCompact, formatCurrency, formatDate, sumTokenUsage, tokenTotal } from "../lib/format";
import { summarizeProviderAccountUsage } from "../lib/providerUsage";
import { dataFilterAgents, defaultDataAgents } from "../lib/sourceStatus";
import { useUiStore } from "../store";
import type { DailyUsagePoint, HourlyUsagePoint, Locale, RangeKey, TaskSummary } from "../types";

const COMPARISON_COLORS = [
  "var(--chart-1)",
  "var(--chart-2)",
  "var(--chart-3)",
  "var(--agent-cursor)",
  "var(--warning)",
  "var(--agent-hermes)",
  "var(--agent-zcode)",
  "var(--red)",
  "var(--chart-4)",
];

type DailyTotal = { date: string; tokens: number; activeSeconds: number; sessions: number };

function durationParts(seconds: number) {
  const safeSeconds = Math.max(0, Math.floor(seconds));
  return {
    hours: Math.floor(safeSeconds / 3_600),
    minutes: Math.floor((safeSeconds % 3_600) / 60),
  };
}

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
  if (agent === "zcode") return colors.agent[6];
  return colors.agent[(fallbackIndex + 3) % colors.agent.length];
}

export function trendOption(series: AgentTrendSeries[], colors: ReturnType<typeof useChartColors>, locale: Locale, hourly: boolean): EChartsCoreOption {
  return {
    grid: { left: 10, right: 18, top: 12, bottom: 26, containLabel: true },
    tooltip: { ...chartTooltip(colors), trigger: "axis", valueFormatter: (value: unknown) => formatCompact(Number(value), locale) },
    xAxis: { type: "category", boundaryGap: false, data: series[0]?.buckets ?? [], axisLine: { lineStyle: { color: colors.hairline } }, axisTick: { show: false }, axisLabel: { color: colors.textTertiary, fontSize: 11, hideOverlap: true, formatter: (value: string) => hourly ? value.slice(11, 16) : value.slice(5) } },
    yAxis: { type: "value", splitNumber: 3, axisLabel: { color: colors.textTertiary, fontSize: 11, formatter: (value: number) => formatCompact(value, locale) }, splitLine: { lineStyle: { color: colors.hairline } } },
    series: series.map((item, index) => ({
      name: agentName(item.agent),
      type: "line",
      // Keep plotted points tied to observed usage. ECharts still smooths the
      // curve visually, but preprocessing the values would make the line,
      // y-axis, and tooltip disagree on the actual Token total.
      data: item.values,
      showSymbol: false,
      smooth: .12,
      smoothMonotone: "x",
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
    series: [{ type: "bar", data: visible.map((item, index) => ({ value: item.value, itemStyle: { color: colors.series[index % colors.series.length], borderRadius: [0, 5, 5, 0] } })), barWidth: 10, label: { show: true, position: "right", color: colors.textTertiary, fontSize: 11, formatter: (item: { value: number }) => formatCompact(item.value, locale) } }],
  };
}

function openTask(task: TaskSummary) {
  const store = useUiStore.getState();
  store.openSessions(task.primarySessionId);
}

export function DataPage({ locale }: { locale: Locale }) {
  const { t } = useTranslation();
  const colors = useChartColors();
  const client = useQueryClient();
  const range = useUiStore((state) => state.range);
  const dataView = useUiStore((state) => state.dataView);
  const openSessions = useUiStore((state) => state.openSessions);
  const closeSessions = useUiStore((state) => state.closeSessions);
  const [selectedTasks, setSelectedTasks] = useState<string[]>([]);
  const [showAllEvents, setShowAllEvents] = useState(false);
  const [agentFilter, setAgentFilter] = useState<string[] | null>(null);
  const overview = useQuery({ queryKey: ["overview", range], queryFn: () => api.overview(range), refetchInterval: 30_000 });
  const sources = useQuery({ queryKey: ["sources"], queryFn: api.sources, refetchInterval: 30_000 });
  const tasks = useQuery({ queryKey: ["tasks", range], queryFn: () => api.tasks(range), refetchInterval: 30_000 });
  const comparison = useQuery({ queryKey: ["comparison", range], queryFn: () => api.comparison(range) });
  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  const cursorDashboardEnabled = settings.data?.credentialsAllowed === "true" && settings.data.cursorDashboardUsage === "true";
  const providers = useQuery({
    queryKey: ["providers"],
    queryFn: api.providers,
    enabled: cursorDashboardEnabled,
    refetchInterval: cursorDashboardEnabled ? 30_000 : false,
  });
  const merge = useMutation({
    mutationFn: (taskIds: string[]) => api.mergeTasks(taskIds),
    onSuccess: async () => {
      setSelectedTasks([]);
      await Promise.all([client.invalidateQueries({ queryKey: ["tasks"] }), client.invalidateQueries({ queryKey: ["overview"] }), client.invalidateQueries({ queryKey: ["sessions"] })]);
    },
  });
  const filterAgents = useMemo(
    () => dataFilterAgents(sources.data ?? []),
    [sources.data],
  );
  const defaultAgents = useMemo(
    () => defaultDataAgents(sources.data ?? [], overview.data?.agents ?? []),
    [overview.data?.agents, sources.data],
  );
  useEffect(() => {
    if (!sources.data || !overview.data) return;
    setAgentFilter((current) => {
      if (current === null) return current;
      const stillValid = current.filter((agent) => filterAgents.includes(agent));
      return stillValid.length ? stillValid : defaultAgents;
    });
  }, [filterAgents, defaultAgents, overview.data, sources.data]);

  const isToday = range === "today";
  const referenceTime = useMemo(() => {
    const value = new Date(overview.data?.generatedAt ?? Date.now());
    return Number.isNaN(value.getTime()) ? new Date() : value;
  }, [overview.data?.generatedAt]);
  const activeAgents = agentFilter ?? defaultAgents;
  const filteredDaily = useMemo(
    () => (overview.data?.daily ?? []).filter((point) => activeAgents.includes(point.agent)),
    [overview.data?.daily, activeAgents],
  );
  const filteredHourly = useMemo(
    () => (overview.data?.hourly ?? []).filter((point) => activeAgents.includes(point.agent)),
    [overview.data?.hourly, activeAgents],
  );
  const series = useMemo(
    () => agentSeries(filteredDaily, filteredHourly, isToday, referenceTime, activeAgents),
    [filteredDaily, filteredHourly, isToday, referenceTime, activeAgents],
  );
  const daily = useMemo(() => groupDaily(filteredDaily, range, referenceTime), [filteredDaily, range, referenceTime]);
  const hourlyActivity = useMemo(() => isToday ? buildHourlyActivity(filteredHourly, "today", referenceTime) : [], [isToday, filteredHourly, referenceTime]);
  const peakActivity = useMemo(() => findPeakActivity(isToday
    ? hourlyActivity.map((item) => ({ period: item.startAt, total: item.total }))
    : daily.map((item) => ({ period: item.date, total: item.tokens }))), [daily, hourlyActivity, isToday]);
  const maxActivity = Math.max(...(isToday ? hourlyActivity.map((item) => item.total) : daily.map((item) => item.tokens)), 1);
  const heatmapRef = useRef<HTMLDivElement>(null);
  const [heatmapIndex, setHeatmapIndex] = useState(0);
  useEffect(() => {
    setHeatmapIndex(0);
  }, [isToday, daily.length, hourlyActivity.length]);
  const cursorAccountUsage = providers.data?.find((provider) => provider.provider === "cursor")?.accountUsage ?? undefined;
  const cursorAccountSummary = useMemo(
    () => cursorAccountUsage ? summarizeProviderAccountUsage(cursorAccountUsage, range, referenceTime) : undefined,
    [cursorAccountUsage, range, referenceTime],
  );
  const activateHeatmap = (index: number) => {
    setHeatmapIndex(index);
    requestAnimationFrame(() => focusHeatmapIndex(heatmapRef.current, index));
  };

  if (dataView === "sessions") {
    return <SessionsWorkspace locale={locale} embedded onBack={closeSessions} />;
  }

  if (overview.isLoading || sources.isLoading || tasks.isLoading) return <LoadingState />;
  if (overview.isError || !overview.data || sources.isError || !sources.data || tasks.isError || !tasks.data) {
    return <ErrorState retry={() => void Promise.all([overview.refetch(), sources.refetch(), tasks.refetch()])} />;
  }
  const data = overview.data;
  const filteredAgents = data.agents.filter((item) => activeAgents.includes(item.label));
  const displayedAgents = activeAgents.map((agent) => filteredAgents.find((item) => item.label === agent) ?? { id: agent, label: agent, value: 0 });
  const filteredTasks = tasks.data.filter((task) => activeAgents.includes(task.agent));
  const visibleTasks = showAllEvents ? filteredTasks : filteredTasks.slice(0, 12);
  const filteredComparison = (comparison.data ?? []).filter((item) => item.groupKind !== "agent" || activeAgents.includes(item.agent));
  const ledgerSessions = filteredDaily.reduce((sum, point) => sum + point.sessionCount, 0);
  const ledgerUsage = sumTokenUsage(filteredDaily.map((point) => point.usage));
  const ledgerTokens = tokenTotal(ledgerUsage);
  const ledgerCacheTokens = cacheTokenTotal(ledgerUsage);
  const ledgerSeconds = filteredDaily.reduce((sum, point) => sum + point.activeSeconds, 0);
  const ledgerDays = new Set(filteredDaily.filter((point) => tokenTotal(point.usage) > 0 || point.sessionCount > 0).map((point) => point.date)).size;
  const activeDuration = durationParts(ledgerSeconds || data.totals.activeSeconds);
  const toggleTask = (id: string, selected: boolean) => setSelectedTasks((current) => selected ? [...new Set([...current, id])] : current.filter((item) => item !== id));
  const toggleAgent = (agent: string) => {
    setAgentFilter((current) => {
      const base = current ?? defaultAgents;
      if (base.includes(agent)) {
        const next = base.filter((item) => item !== agent);
        return next.length ? next : base;
      }
      return [...base, agent];
    });
  };
  const rangeText = t(`ranges.${range}`);
  const agentTokenText = (agent: string, value: number) => agent === "cursor"
    ? cursorDashboardEnabled
      ? cursorAccountSummary
        ? t("cursorUsage.accountValue", { value: formatCompact(cursorAccountSummary.totalTokens, locale) })
        : providers.isLoading || (providers.isFetching && !cursorAccountUsage)
          ? t("cursorUsage.loadingShort")
          : t("cursorUsage.unavailableShort")
      : t("metrics.notRecorded")
    : formatCompact(value, locale);
  const activeHours = hourlyActivity.filter((item) => item.total > 0);
  const hourWindow = activeHours.length ? `${activeHours[0].startAt.slice(11, 16)} — ${activeHours[activeHours.length - 1].startAt.slice(11, 16)}` : t("data.noHourlyActivity");
  const peakActivityPeriod = peakActivity
    ? isToday
      ? `${formatDate(peakActivity.period.slice(0, 10), locale, "short")} · ${peakActivity.period.slice(11, 16)}`
      : formatDate(peakActivity.period, locale, "short")
    : t("data.noPeakActivity");

  return (
    <div className="page data-page">
      <header className="data-header">
        <div><span className="eyebrow"><BarChart3 size={13} />{t("data.eyebrow")}</span><h1>{locale === "zh-CN" ? <>你与 Agent<br />一起完成的工作</> : t("data.title")}</h1><p>{t("data.description")}</p></div>
        <div className="data-header-actions">
          <div className="data-agent-chips" role="group" aria-label={t("data.agentFilter")}>
            {filterAgents.length ? filterAgents.map((agent) => {
              const active = activeAgents.includes(agent);
              return (
                <button
                  key={agent}
                  type="button"
                  className={active ? "active" : ""}
                  aria-pressed={active}
                  aria-label={agentName(agent)}
                  onClick={() => toggleAgent(agent)}
                >
                  <AgentBadge agent={agent} compact />
                </button>
              );
            }) : <span className="data-source-empty">{t("data.sourcesNoneSelected")}</span>}
          </div>
          <RangePicker />
        </div>
      </header>

      <SourceCapabilityLegend locale={locale} />

      <section className="data-ledger" aria-label={t("data.summary")}>
        <div><span>{t("metrics.sessions")}</span><strong>{formatCompact(ledgerSessions || data.totals.sessionCount, locale)}</strong><small>{rangeText}</small></div>
        <div className="duration-metric">
          <span>{t("metrics.duration")}</span>
          <strong className="data-duration-value">
            <span><b>{formatCompact(activeDuration.hours, locale)}</b><em>{t("metrics.hours")}</em></span>
            <span><b>{formatCompact(activeDuration.minutes, locale)}</b><em>{t("metrics.minutes")}</em></span>
          </strong>
          <small>{ledgerDays || data.totals.activeDays} {t("metrics.activeDays").toLowerCase()}</small>
        </div>
        <div className="featured">
          <span>{t("metrics.tokens")}</span>
          <strong>{formatCompact(ledgerTokens, locale)}</strong>
          <div className="data-token-breakdown">
            <span className="input"><i /><em>{t("metrics.input")}</em><b>{formatCompact(ledgerUsage.inputTokens, locale)}</b></span>
            <span className="output"><i /><em>{t("metrics.output")}</em><b>{formatCompact(ledgerUsage.outputTokens, locale)}</b></span>
            <span className="cache"><i /><em>{t("metrics.cache")}</em><b>{formatCompact(ledgerCacheTokens, locale)}</b></span>
          </div>
          <small className="data-token-footnote" title={t("data.localTokenFootnote")}>{t("data.localTokenFootnote")}</small>
        </div>
        <div className="cost"><span>{t("metrics.cost")}</span><strong>{data.totals.estimatedCostUsd !== undefined ? formatCurrency(data.totals.estimatedCostUsd, locale) : t("metrics.unavailable")}</strong><small>{t("data.observedLocally")}</small></div>
        <div><span>{t("metrics.activeDays")}</span><strong>{formatCompact(ledgerDays || data.totals.activeDays, locale)}</strong><small>{t("data.observedLocally")}</small></div>
      </section>

      <CursorAccountUsagePanel locale={locale} range={range} compact />

      <section className="data-panel trend-panel">
        <header className="panel-heading"><div><h2>{t("data.trend")}</h2></div><span className="panel-kicker"><Sparkles size={13} />{t("data.combinedAgents")}</span></header>
        <div className="combined-trend">
          {series.length ? <>
            <div className="trend-series-legend">{series.map((item, index) => <div key={item.agent}><i style={{ background: agentColor(item.agent, colors, index) }} /><strong>{agentName(item.agent)}</strong><small>{agentTokenText(item.agent, item.total)}</small></div>)}</div>
            <EChart option={trendOption(series, colors, locale, isToday)} ariaLabel={t("data.trend")} style={{ height: 260 }} />
          </> : <EmptyState title={t("data.noUsage")} body={t("data.noUsageBody")} />}
        </div>
      </section>

      <div className="data-analysis-grid">
        <section className="data-panel activity-panel">
          <header className="panel-heading"><div><h2>{t("data.activity")}</h2></div><CalendarDays size={17} /></header>
          <div className="activity-summary">
            <div className="activity-summary-main">
              <strong>{isToday ? activeHours.length : (ledgerDays || data.totals.activeDays)}</strong>
              <span>{isToday ? t("data.activeHours") : t("metrics.activeDays")}</span>
              <small>{isToday ? hourWindow : daily.length ? `${formatDate(daily[0].date, locale, "short")} — ${formatDate(daily[daily.length - 1].date, locale, "short")}` : rangeText}</small>
            </div>
            <div className="activity-peak" aria-label={t("data.peakActivity")}>
              <span>{t("data.peakActivity")}</span>
              <strong>{peakActivity ? `${formatCompact(peakActivity.total, locale)} ${t("metrics.tokens")}` : t("metrics.unavailable")}</strong>
              <small>{peakActivityPeriod}</small>
            </div>
          </div>
          {isToday ? <>
            <div ref={heatmapRef} className="activity-heatmap hourly" style={{ gridTemplateColumns: `repeat(${Math.max(1, hourlyActivity.length)}, minmax(7px, 1fr))` }}>
              {hourlyActivity.map((item, index) => (
                <HeatmapCell
                  key={item.key}
                  index={index}
                  total={hourlyActivity.length}
                  active={heatmapIndex === index}
                  onActivate={activateHeatmap}
                  style={{ opacity: .12 + .88 * Math.sqrt(item.total / maxActivity) }}
                  tooltip={`${item.startAt.slice(11, 16)} · ${formatCompact(item.total, locale)} Token`}
                  ariaLabel={`${item.startAt.slice(11, 16)} ${formatCompact(item.total, locale)} Token`}
                />
              ))}
            </div>
            {hourlyActivity.length ? <div className="hour-axis"><span>{hourlyActivity[0].startAt.slice(11, 16)}</span><span>{hourlyActivity[Math.floor(hourlyActivity.length / 2)].startAt.slice(11, 16)}</span><span>{hourlyActivity[hourlyActivity.length - 1].startAt.slice(11, 16)}</span></div> : null}
          </> : <div ref={heatmapRef} className="activity-heatmap" style={{ gridTemplateColumns: `repeat(${Math.max(1, Math.ceil(daily.length / 7))}, minmax(7px, 1fr))` }}>
            {daily.map((item, index) => (
              <HeatmapCell
                key={item.date}
                index={index}
                total={daily.length}
                active={heatmapIndex === index}
                onActivate={activateHeatmap}
                style={{ opacity: .15 + .85 * Math.sqrt(item.tokens / maxActivity) }}
                tooltip={`${formatDate(item.date, locale)} · ${formatCompact(item.tokens, locale)} Token · ${item.sessions} ${t("metrics.sessions")}`}
                ariaLabel={`${formatDate(item.date, locale)} ${formatCompact(item.tokens, locale)} Token`}
              />
            ))}
          </div>}
        </section>
        <section className="data-panel distribution-panel">
          <header className="panel-heading"><div><h2>{t("data.distribution")}</h2></div></header>
          <div className="distribution-layout">
            <EChart option={distributionOption(displayedAgents, data.models, colors, locale, { agent: t("data.agentLegend"), model: t("data.modelLegend") })} ariaLabel={t("data.distribution")} style={{ height: 250 }} />
            <aside className="distribution-legend">
              <div><strong>{t("data.agentLegend")}</strong>{displayedAgents.map((item, index) => <span key={item.id}><i style={{ background: agentColor(item.label, colors, index) }} /><b>{agentName(item.label)}</b><small>{agentTokenText(item.label, item.value)}</small></span>)}</div>
              <div><strong>{t("data.modelLegend")}</strong>{data.models.slice(0, 6).map((item, index) => <span key={item.id}><i style={{ background: colors.model[index % colors.model.length] }} /><b title={item.label}>{item.label}</b><small>{formatCompact(item.value, locale)}</small></span>)}</div>
            </aside>
          </div>
        </section>
        <section className="data-panel tools-panel">
          <header className="panel-heading"><div><h2>{t("data.tools")}</h2></div><Workflow size={17} /></header>
          <EChart option={toolOption(data.tools, colors, locale)} ariaLabel={t("data.tools")} style={{ height: 240 }} />
        </section>
      </div>

      <section className="data-panel skill-usage-panel">
        <header className="panel-heading">
          <div><h2>{t("data.skillUsage")}</h2></div>
          <span className="skill-evidence-badge"><Blocks size={14} />{t("data.skillExplicitOnly")}</span>
        </header>
        <div className="skill-usage-grid">
          <article className="skill-ranking most-used">
            <header><span>{t("data.skillMostUsed")}</span><strong>{t("data.skillUsedTotal", { count: data.skills.usedCount })}</strong></header>
            {data.skills.mostUsed.length ? data.skills.mostUsed.map((skill, index) => (
              <div key={skill.name}><i>{String(index + 1).padStart(2, "0")}</i><strong>{skill.name}</strong><span>{t("data.skillInvocationCount", { count: skill.invocationCount })}</span></div>
            )) : <p>{t("data.skillNoUsage")}</p>}
          </article>
          <article className="skill-ranking least-used">
            <header><span>{t("data.skillLeastUsed")}</span><strong>{t("data.skillItemCount", { count: data.skills.leastUsed.length })}</strong></header>
            {data.skills.leastUsed.length ? data.skills.leastUsed.map((skill) => (
              <div key={skill.name}><i>↓</i><strong>{skill.name}</strong><span>{t("data.skillInvocationCount", { count: skill.invocationCount })}</span></div>
            )) : <p>{t("data.skillNoUsage")}</p>}
          </article>
          <article className="skill-unused">
            <header><span>{t("data.skillNotRecorded")}</span><strong>{data.skills.installedWithoutUsage.length}</strong></header>
            <p>{t("data.skillNotRecordedBody", { count: data.skills.installedCount })}</p>
            <div>
              {data.skills.installedWithoutUsage.slice(0, 12).map((skill) => <span key={skill}>{skill}</span>)}
              {data.skills.installedWithoutUsage.length > 12 ? <span>+{data.skills.installedWithoutUsage.length - 12}</span> : null}
            </div>
          </article>
        </div>
      </section>

      <section className="data-panel comparison-panel">
        <header className="panel-heading"><div><h2>{t("insights.comparison")}</h2></div><Scale size={17} /></header>
        {comparison.isLoading ? <LoadingState /> : comparison.isError || !comparison.data ? (
          <ErrorState retry={() => void comparison.refetch()} />
        ) : (() => {
          const maxTokens = Math.max(...filteredComparison.map((entry) => tokenTotal(entry.usage)), 1);
          return (
            <div className="comparison-list">
              {filteredComparison.slice(0, 8).map((item, index) => (
                <div key={item.id} style={{ "--comparison-color": COMPARISON_COLORS[index] } as CSSProperties}>
                  <span className="comparison-label">
                    <strong><i className="comparison-dot" />{item.label}</strong>
                    <small>{item.groupKind}</small>
                  </span>
                  <div className="comparison-bar"><i style={{ width: `${(tokenTotal(item.usage) / maxTokens) * 100}%` }} /></div>
                  <span><strong>{formatCompact(tokenTotal(item.usage), locale)}</strong><small>{t("metrics.tokens")}</small></span>
                </div>
              ))}
            </div>
          );
        })()}
      </section>

      <section className="events-section">
        <header className="section-heading events-heading"><div><h2>{t("data.events")}</h2></div><div className="event-summary"><span><strong>{filteredTasks.length}</strong>{t("metrics.tasks")}</span></div></header>
        {selectedTasks.length ? <div className="task-merge-bar"><span>{t("task.selected", { count: selectedTasks.length })}</span><button className="button secondary" onClick={() => merge.mutate(selectedTasks)} disabled={selectedTasks.length < 2 || merge.isPending}><Merge size={13} />{t("task.merge")}</button></div> : null}
        {visibleTasks.length ? <div className="event-grid">{visibleTasks.map((task) => <WorkEventCard key={task.id} task={task} locale={locale} selected={selectedTasks.includes(task.id)} onSelect={(value) => toggleTask(task.id, value)} onOpen={() => openTask(task)} onAcceptSuggestion={task.suggestedTaskId ? () => merge.mutate([task.suggestedTaskId!, task.id]) : undefined} />)}</div> : <EmptyState title={t("data.noEvents")} body={t("data.noEventsBody")} />}
        <footer className="events-footer">
          {filteredTasks.length > 12 ? <button className="button subtle" onClick={() => setShowAllEvents((value) => !value)}>{showAllEvents ? t("data.showLess") : t("data.showAll", { count: filteredTasks.length })}</button> : <span />}
          <button className="inline-link" onClick={() => openSessions()}>{t("data.openSessions")}<ArrowRight size={14} /></button>
        </footer>
      </section>
    </div>
  );
}
