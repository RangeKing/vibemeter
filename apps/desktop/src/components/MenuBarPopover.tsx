import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Activity, ArrowUpRight, Coins, Gauge, Power, RefreshCw, Settings, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import appIconUrl from "../../src-tauri/icons/vibemeter-icon-source.png";
import { api } from "../lib/api";
import { formatCompact, formatCurrency, tokenTotal } from "../lib/format";
import type { Locale, RateWindow } from "../types";
import { focusHeatmapIndex, HeatmapCell } from "./HeatmapCell";
import { ErrorState, LoadingState } from "./ui";

function resetTime(window: RateWindow, locale: Locale): string | undefined {
  if (window.resetAt) {
    const date = new Date(window.resetAt);
    if (!Number.isNaN(date.getTime())) {
      return new Intl.DateTimeFormat(locale, {
        month: "short",
        day: "numeric",
        weekday: "short",
        hour: "2-digit",
        minute: "2-digit",
      }).format(date);
    }
  }
  return window.resetDescription;
}

function providerName(provider: string): string {
  if (provider === "claude") return "Claude";
  if (provider === "codex") return "Codex";
  if (provider === "cursor") return "Cursor";
  return provider;
}

export function MenuBarPopover({ locale }: { locale: Locale }) {
  const { t } = useTranslation();
  const client = useQueryClient();
  const [refreshing, setRefreshing] = useState(false);
  const refreshedOnOpen = useRef(false);
  const heatmapRef = useRef<HTMLDivElement>(null);
  const [heatmapIndex, setHeatmapIndex] = useState(0);
  const snapshot = useQuery({ queryKey: ["menu-snapshot"], queryFn: api.menuSnapshot, refetchInterval: 30_000 });
  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  useEffect(() => {
    if (refreshedOnOpen.current || !settings.data) return;
    refreshedOnOpen.current = true;
    // Refresh health and quota using the persisted permission state. Passing a
    // synthetic disabled state here would also clear the optional in-memory
    // Cursor Dashboard snapshot.
    void api.refreshProviders(
      settings.data.credentialsAllowed === "true",
      settings.data.cursorDashboardUsage === "true",
    ).finally(() => void client.invalidateQueries({ queryKey: ["menu-snapshot"] }));
  }, [client, settings.data]);
  const refresh = async () => {
    setRefreshing(true);
    try {
      await Promise.all([
        api.refreshIndex(false),
        api.refreshProviders(
          settings.data?.credentialsAllowed === "true",
          settings.data?.cursorDashboardUsage === "true",
        ),
      ]);
      await client.invalidateQueries({ queryKey: ["menu-snapshot"] });
    } finally { setRefreshing(false); }
  };
  const open = async (settingsPage = false) => { if (settingsPage) await api.showSettings(); else await api.showMain(); await api.hideMenu(); };
  if (snapshot.isLoading) return <div className="menubar-root"><LoadingState /></div>;
  if (snapshot.isError || !snapshot.data) return <div className="menubar-root"><ErrorState retry={() => void snapshot.refetch()} /></div>;
  const data = snapshot.data;
  const heatmap = [...data.heatmap.reduce((days, point) => {
    const current = days.get(point.date) ?? { value: 0, sessions: 0 };
    current.value += tokenTotal(point.usage);
    current.sessions += point.sessionCount;
    days.set(point.date, current);
    return days;
  }, new Map<string, { value: number; sessions: number }>())].sort(([left], [right]) => left.localeCompare(right));
  const maxDay = Math.max(...heatmap.map(([, item]) => item.value), 1);
  const activateHeatmap = (index: number) => {
    setHeatmapIndex(index);
    requestAnimationFrame(() => focusHeatmapIndex(heatmapRef.current, index));
  };
  const cache = data.todayUsage.cacheReadTokens + data.todayUsage.cacheWriteTokens + data.todayUsage.cacheWrite1hTokens;
  const provider = (name: string) => data.providers.find((item) => item.provider === name);
  const providerState = (name: string) => provider(name)?.health.state ?? "unknown";
  const providerLabel = (state: string) => state === "operational" ? "operational" : state === "minor" ? "degraded" : state === "major" || state === "critical" ? "outage" : "unknown";
  const quotaWindows = data.providers.flatMap((provider) => provider.windows.map((window) => ({ provider, window }))).slice(0, 3);
  return (
    <div className="menubar-root">
      <header className="menubar-header" data-tauri-drag-region><div><span className="menu-logo"><img src={appIconUrl} alt="" /></span><strong>{t("app.name")}</strong></div><button onClick={() => void api.hideMenu()} aria-label={t("actions.close")}><X size={15} /></button></header>
      <section className="menu-token-hero">
        <span>{t("menubar.todayUsage")}</span>
        <strong>{formatCompact(tokenTotal(data.todayUsage), locale)}</strong>
        <div className="menu-token-legend">
          <span className="input"><i />{t("metrics.input")} <b>{formatCompact(data.todayUsage.inputTokens, locale)}</b></span>
          <span className="output"><i />{t("metrics.output")} <b>{formatCompact(data.todayUsage.outputTokens, locale)}</b></span>
          <span className="cache"><i />{t("metrics.cache")} <b>{formatCompact(cache, locale)}</b></span>
        </div>
      </section>
      <section className="menu-activity">
        <header><span><Activity size={14} />{t("menubar.recentActivity")}</span>{data.todayCostUsd !== undefined ? <span><Coins size={12} />{formatCurrency(data.todayCostUsd, locale)}</span> : null}</header>
        <div ref={heatmapRef} className="menu-heatmap">
          {heatmap.map(([date, item], index) => {
            const detail = `${new Intl.DateTimeFormat(locale, { month: "short", day: "numeric" }).format(new Date(`${date}T12:00:00`))} · ${formatCompact(item.value, locale)} Token · ${item.sessions} ${t("metrics.sessions")}`;
            return (
              <HeatmapCell
                key={date}
                className="heatmap-cell"
                index={index}
                total={heatmap.length}
                active={heatmapIndex === index}
                onActivate={activateHeatmap}
                style={{ opacity: .14 + .86 * Math.sqrt(item.value / maxDay) }}
                tooltip={detail}
                ariaLabel={detail}
              />
            );
          })}
        </div>
      </section>
      <section className="menu-quota-card">
        <header><Gauge size={14} /><span>{t("menubar.quota")}</span></header>
        {settings.data?.credentialsAllowed === "true" ? <div className="menu-quota-list">{quotaWindows.map(({ provider, window }) => {
          const remaining = window.usedPercent === undefined ? undefined : Math.max(0, Math.min(100, 100 - window.usedPercent));
          const reset = resetTime(window, locale);
          const resetLabel = reset ? t("menubar.resetsAt", { time: reset }) : t("menubar.resetUnknown");
          return <div className={`menu-quota-row ${remaining !== undefined && remaining < 20 ? "critical" : remaining !== undefined && remaining < 50 ? "warning" : ""}`} key={`${provider.provider}-${window.id}`}>
            <div className="menu-quota-copy"><span>{providerName(provider.provider)} · {t(window.label, { defaultValue: window.label })}</span><strong>{remaining === undefined ? t("metrics.unavailable") : t("menubar.remaining", { value: Math.round(remaining) })}</strong></div>
            <div className="menu-quota-track" aria-hidden="true"><i style={{ width: `${remaining ?? 0}%` }} /></div>
            <small title={resetLabel}>{resetLabel}</small>
          </div>;
        })}</div> : <button className="menu-enable-quota" onClick={() => void open(true)}><span><strong>{t("menubar.noQuota")}</strong><small>{t("menubar.enableInSettings")}</small></span><ArrowUpRight size={15} /></button>}
      </section>
      <section className="menu-provider-status">
        <header>
          <span><Activity size={14} />{t("menubar.providerStatus")}</span>
          {data.indexStatus.running ? <span className="menu-index"><span className="pulse-dot" />{t("menubar.indexing", { processed: data.indexStatus.processedFiles, total: data.indexStatus.discoveredFiles })}</span> : null}
        </header>
        <div>{["claude", "codex", "cursor"].map((name) => {
          const item = provider(name);
          const state = providerState(name);
          const displayName = name === "claude" ? "Anthropic" : name === "codex" ? "OpenAI" : "Cursor";
          return <span key={name}><i className={state} /><strong>{displayName}</strong><small>{t(`provider.${providerLabel(state)}`)}</small>{item?.health.statusUrl ? <button className="menu-provider-link" onClick={() => void api.openProviderStatus(name)}>{t("menubar.viewStatus")}</button> : null}</span>;
        })}</div>
      </section>
      <footer className="menu-actions"><button className="menu-open-app" onClick={() => void open()}>{t("actions.openApp")}</button><span /><button onClick={() => void refresh()} aria-label={t("actions.refresh")}><RefreshCw className={refreshing ? "spin" : ""} size={15} /></button><button onClick={() => void open(true)} aria-label={t("actions.settings")}><Settings size={15} /></button><button className="danger" onClick={() => void api.quit()} aria-label={t("actions.quit")}><Power size={15} /></button></footer>
    </div>
  );
}
