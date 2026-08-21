import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { ArrowRight, BarChart3, Database, GitBranch, HardDrive, Languages, Laptop, LoaderCircle, LockKeyhole, PanelTop, Power, RadioTower, RefreshCw, ScanSearch, ShieldAlert, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import desktopPackage from "../../package.json";
import { AgentBadge, ErrorState, LoadingState, PageHeader, Toggle } from "../components/ui";
import { api } from "../lib/api";
import { refreshHistoryIndex } from "../lib/indexRefresh";
import { detectedDataAgents, parseDataPageAgents, serializeDataPageAgents, sourceCapabilityNameGroups } from "../lib/sourceStatus";
import { useUiStore } from "../store";
import type { AppSettings, DiagnosticRetentionStatus, Locale, Theme } from "../types";

export function DiagnosticRetentionControl({
  status,
  locale,
  pending,
  loading,
  hasError,
  clearCount,
  onToggle,
  onClear,
}: {
  status?: DiagnosticRetentionStatus;
  locale: Locale;
  pending: boolean;
  loading: boolean;
  hasError: boolean;
  clearCount: number | null;
  onToggle: (enabled: boolean) => void;
  onClear: () => void;
}) {
  const { t } = useTranslation();
  const formatTime = (value?: string) => value
    ? new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "short" }).format(new Date(value))
    : t("settings.diagnosticNotStarted");
  return (
    <div className="setting-row multiline diagnostic-retention-row">
      <div>
        <strong>{t("settings.diagnosticMode")}</strong>
        <p>{t("settings.diagnosticModeBody")}</p>
        {status ? (
          <dl className="diagnostic-retention-details">
            <div><dt>{t("settings.diagnosticState")}</dt><dd>{t(`settings.diagnosticStates.${status.state}`)}</dd></div>
            <div><dt>{t("settings.diagnosticLocation")}</dt><dd>{status.storageLocation}</dd></div>
            <div><dt>{t("settings.diagnosticStarted")}</dt><dd>{formatTime(status.startedAt)}</dd></div>
            <div><dt>{t("settings.diagnosticExpires")}</dt><dd>{formatTime(status.expiresAt)}</dd></div>
            <div><dt>{t("settings.diagnosticCount")}</dt><dd>{status.retainedEnvelopes}</dd></div>
          </dl>
        ) : null}
        {hasError ? <p className="setting-error" role="alert">{t("settings.diagnosticUnavailable")}</p> : null}
        {clearCount !== null ? <p className="setting-success" role="status">{t("settings.diagnosticCleared", { count: clearCount })}</p> : null}
      </div>
      <div className="diagnostic-retention-actions">
        <Toggle checked={status?.enabled === true} disabled={pending || loading} onCheckedChange={onToggle} label={t("settings.diagnosticMode")} />
        {(status?.enabled || status?.retainedEnvelopes || status?.state === "unavailable") ? (
          <button className="button secondary" disabled={pending} onClick={onClear}>
            <Trash2 size={13} />{t("settings.diagnosticClear")}
          </button>
        ) : null}
      </div>
    </div>
  );
}

export function SettingsPage({ locale }: { locale: Locale }) {
  const { t } = useTranslation();
  const capabilityNames = sourceCapabilityNameGroups(locale === "zh-CN" ? "、" : ", ");
  const appVersion = desktopPackage.version;
  const client = useQueryClient();
  const setPage = useUiStore((state) => state.setPage);
  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  const projects = useQuery({ queryKey: ["projects"], queryFn: api.projects });
  const sources = useQuery({ queryKey: ["sources"], queryFn: api.sources, refetchInterval: 30_000 });
  const live = useQuery({ queryKey: ["live-snapshot"], queryFn: api.liveSnapshot, refetchInterval: 3_000 });
  const diagnostics = useQuery({ queryKey: ["diagnostic-retention"], queryFn: api.diagnosticRetention, refetchInterval: 60_000 });
  const [loginEnabled, setLoginEnabled] = useState(false);
  const [cursorRefreshPending, setCursorRefreshPending] = useState(false);
  const [historyRefreshPending, setHistoryRefreshPending] = useState(false);
  const [cursorDashboardDraft, setCursorDashboardDraft] = useState<boolean | null>(null);
  const [dataPageAgentsDraft, setDataPageAgentsDraft] = useState<string | null>(null);
  const [dataPageAgentsPending, setDataPageAgentsPending] = useState(false);
  const [agentDetectionPending, setAgentDetectionPending] = useState(false);
  const [diagnosticClearCount, setDiagnosticClearCount] = useState<number | null>(null);
  useEffect(() => { void isEnabled().then(setLoginEnabled).catch(() => setLoginEnabled(false)); }, []);

  const setSetting = async (key: keyof AppSettings, value: string) => {
    const refreshesProviders = key === "credentialsAllowed" || key === "cursorDashboardUsage" || key === "useSystemProxy";
    if (refreshesProviders) setCursorRefreshPending(true);
    if (key === "gitReadAllowed") setHistoryRefreshPending(true);
    try {
      await api.setSetting(key, value);
      if (key === "credentialsAllowed" && value === "false") {
        await api.setSetting("cursorDashboardUsage", "false");
      }
      if (refreshesProviders) {
        const credentialsAllowed = key === "credentialsAllowed"
          ? value === "true"
          : settings.data?.credentialsAllowed === "true";
        const cursorDashboardUsageEnabled = key === "cursorDashboardUsage"
          ? value === "true"
          : credentialsAllowed && settings.data?.cursorDashboardUsage === "true";
        const useSystemProxy = key === "useSystemProxy"
          ? value === "true"
          : settings.data?.useSystemProxy === "true";
        await api.refreshProviders(credentialsAllowed, cursorDashboardUsageEnabled, useSystemProxy);
      }
      if (key === "gitReadAllowed") {
        await refreshHistoryIndex({
          start: api.refreshIndex,
          status: api.indexStatus,
          completed: async () => {
            await Promise.all([
              client.invalidateQueries({ queryKey: ["index-status"] }),
              client.invalidateQueries({ queryKey: ["sessions"] }),
              client.invalidateQueries({ queryKey: ["session"] }),
            ]);
          },
        });
      } else if (key === "vctiPromptStructure") {
        await api.refreshIndex(true);
      }
      await Promise.all([
        client.invalidateQueries({ queryKey: ["settings"] }),
        client.invalidateQueries({ queryKey: ["providers"] }),
        client.invalidateQueries({ queryKey: ["menu-snapshot"] }),
        client.invalidateQueries({ queryKey: ["vcti"] }),
        client.invalidateQueries({ queryKey: ["overview"] }),
        client.invalidateQueries({ queryKey: ["insights"] }),
      ]);
    } finally {
      if (refreshesProviders) setCursorRefreshPending(false);
      if (key === "gitReadAllowed") setHistoryRefreshPending(false);
    }
  };
  const setCursorDashboard = (value: boolean) => {
    setCursorDashboardDraft(value);
    void setSetting("cursorDashboardUsage", String(value))
      .catch(() => undefined)
      .finally(() => setCursorDashboardDraft(null));
  };
  const setLogin = async (value: boolean) => {
    if (value) await enable(); else await disable();
    setLoginEnabled(await isEnabled());
    await setSetting("launchAtLogin", String(value));
  };
  const exclude = useMutation({
    mutationFn: async ({ hash, excluded }: { hash: string; excluded: boolean }) => excluded ? api.includeProject(hash) : api.excludeProject(hash),
    onSuccess: async () => { await Promise.all([client.invalidateQueries({ queryKey: ["projects"] }), client.invalidateQueries({ queryKey: ["sessions"] })]); },
  });
  const clearData = useMutation({
    mutationFn: api.clearLocalData,
    onSuccess: async () => { await client.invalidateQueries(); },
  });
  const repairHooks = useMutation({
    mutationFn: api.repairLiveHooks,
    onSuccess: async () => {
      await api.setSetting("liveHooksEnabled", "true");
      await Promise.all([
        client.invalidateQueries({ queryKey: ["live-snapshot"] }),
        client.invalidateQueries({ queryKey: ["live-activity"] }),
        client.invalidateQueries({ queryKey: ["settings"] }),
      ]);
    },
  });
  const setDiagnostics = useMutation({
    mutationFn: api.setDiagnosticRetention,
    onSuccess: (status) => {
      client.setQueryData(["diagnostic-retention"], status);
      setDiagnosticClearCount(null);
    },
    onError: () => { void diagnostics.refetch(); },
  });
  const clearDiagnostics = useMutation({
    mutationFn: api.clearDiagnosticRetention,
    onSuccess: (result) => {
      client.setQueryData(["diagnostic-retention"], result.status);
      setDiagnosticClearCount(result.removed);
    },
    onError: () => { void diagnostics.refetch(); },
  });

  if (settings.isLoading || projects.isLoading || sources.isLoading) return <LoadingState />;
  if (settings.isError || !settings.data || projects.isError || sources.isError || !sources.data) return <ErrorState retry={() => void Promise.all([settings.refetch(), projects.refetch(), sources.refetch()])} />;
  const data = settings.data;
  const detectedAgents = detectedDataAgents(sources.data);
  const configuredDataAgents = parseDataPageAgents(dataPageAgentsDraft ?? data.dataPageAgents);
  const autoDataPageAgents = configuredDataAgents === undefined;
  const selectedDataAgents = new Set(configuredDataAgents ?? detectedAgents);
  const theme = data.theme as Theme;
  const diagnosticStatus = diagnostics.data;
  const diagnosticPending = setDiagnostics.isPending || clearDiagnostics.isPending;
  const persistDataPageAgents = async (value: string) => {
    const previous = dataPageAgentsDraft;
    setDataPageAgentsDraft(value);
    setDataPageAgentsPending(true);
    try {
      await setSetting("dataPageAgents", value);
    } catch {
      setDataPageAgentsDraft(previous);
    } finally {
      setDataPageAgentsPending(false);
    }
  };
  const toggleDataPageAgent = (agent: string, checked: boolean) => {
    const base = autoDataPageAgents ? detectedAgents : [...selectedDataAgents];
    const next = checked ? [...base, agent] : base.filter((item) => item !== agent);
    void persistDataPageAgents(serializeDataPageAgents(next));
  };
  const setDataPageAuto = (enabled: boolean) => {
    void persistDataPageAgents(enabled ? "auto" : serializeDataPageAgents(detectedAgents));
  };
  const detectAgents = async () => {
    setAgentDetectionPending(true);
    try {
      await refreshHistoryIndex({
        start: api.refreshIndex,
        status: api.indexStatus,
        force: true,
        completed: async () => {
          await Promise.all([
            sources.refetch(),
            client.invalidateQueries({ queryKey: ["overview"] }),
            client.invalidateQueries({ queryKey: ["settings"] }),
          ]);
        },
      });
    } finally {
      setAgentDetectionPending(false);
    }
  };
  const toggleDiagnostics = (enabled: boolean) => {
    if (enabled) {
      if (window.confirm(t("settings.diagnosticEnableConfirm"))) setDiagnostics.mutate(true);
      return;
    }
    if (window.confirm(t("settings.diagnosticClearConfirm"))) clearDiagnostics.mutate();
  };
  return (
    <div className="page settings-page">
      <PageHeader title={t("settings.title")} description={t("settings.description")} />
      <div className="settings-stack">
        <section className="settings-section">
          <header><Languages size={17} /><div><h2>{t("settings.general")}</h2></div></header>
          <div className="setting-row"><div><strong>{t("settings.language")}</strong></div><select value={data.locale} onChange={(event) => void setSetting("locale", event.target.value)}><option value="system">{t("settings.systemLanguage")}</option><option value="zh-CN">简体中文</option><option value="en-US">English</option></select></div>
          <div className="setting-row"><div><strong>{t("settings.appearance")}</strong></div><div className="segmented compact"><button className={theme === "system" ? "active" : ""} onClick={() => void setSetting("theme", "system")}><Laptop size={13} />{t("settings.systemTheme")}</button><button className={theme === "light" ? "active" : ""} onClick={() => void setSetting("theme", "light")}>{t("share.light")}</button><button className={theme === "dark" ? "active" : ""} onClick={() => void setSetting("theme", "dark")}>{t("share.dark")}</button></div></div>
        </section>

        <section className="settings-section">
          <header><Database size={17} /><div><h2>{t("settings.sources")}</h2><p>{t("settings.sourcesBody")}</p></div></header>
          <div className="setting-row multiline">
            <div><strong>{t("settings.manageSources")}</strong><p>{t("settings.manageSourcesBody")}</p></div>
            <button className="button secondary" onClick={() => setPage("sources")}>{t("settings.openSources")}<ArrowRight size={13} /></button>
          </div>
        </section>

        <section className="settings-section">
          <header><BarChart3 size={17} /><div><h2>{t("settings.dataPageAgents")}</h2><p>{t("settings.dataPageAgentsBody")}</p></div></header>
          <div className="setting-row multiline">
            <div><strong>{t("settings.dataPageAgentsAuto")}</strong><p>{t("settings.dataPageAgentsAutoBody")}</p></div>
            <Toggle checked={autoDataPageAgents} disabled={dataPageAgentsPending} onCheckedChange={setDataPageAuto} label={t("settings.dataPageAgentsAuto")} />
          </div>
          <div className="setting-row multiline">
            <div><strong>{t("settings.dataPageAgentsDetect")}</strong><p>{t("settings.dataPageAgentsDetectBody")}</p></div>
            <button className="button secondary" disabled={agentDetectionPending} onClick={() => void detectAgents()}><RefreshCw size={13} className={agentDetectionPending ? "spin" : ""} />{agentDetectionPending ? t("settings.dataPageAgentsDetecting") : t("settings.dataPageAgentsDetect")}</button>
          </div>
          <div className="data-page-agent-options" role="group" aria-label={t("settings.dataPageAgentsList")}>
            {sources.data.map((source) => (
              <label className={`data-page-agent-option ${source.available ? "" : "is-missing"}`} key={source.agent}>
                <AgentBadge agent={source.agent} compact />
                <span><strong>{source.available ? t("settings.dataPageAgentsDetected") : t("settings.dataPageAgentsNotDetected")}</strong><small>{source.available ? t("settings.dataPageAgentsAvailable") : t("settings.dataPageAgentsUnavailable")}</small></span>
                <input
                  type="checkbox"
                  checked={selectedDataAgents.has(source.agent)}
                  disabled={autoDataPageAgents || !source.available || dataPageAgentsPending}
                  aria-label={source.agent}
                  onChange={(event) => toggleDataPageAgent(source.agent, event.target.checked)}
                />
              </label>
            ))}
          </div>
        </section>

        <section className="settings-section">
          <header><RadioTower size={17} /><div><h2>{t("settings.live")}</h2><p>{t("settings.liveBody")}</p></div></header>
          <div className="setting-row multiline"><div><strong>{t("settings.liveHooks")}</strong><p>{t("settings.liveHooksBody", capabilityNames)}</p></div><Toggle checked={data.liveHooksEnabled === "true"} onCheckedChange={(value) => void setSetting("liveHooksEnabled", String(value))} label={t("settings.liveHooks")} /></div>
          <div className="setting-row multiline"><div><strong>{t("settings.notch")}</strong><p>{t("settings.notchBody")}</p></div><Toggle checked={data.notchEnabled === "true"} onCheckedChange={(value) => void setSetting("notchEnabled", String(value))} label={t("settings.notch")} /></div>
          <div className="setting-row multiline"><div><strong>{t("settings.menuBar")}</strong><p>{t("settings.menuBarBody")}</p></div><Toggle checked={data.menuBarEnabled === "true"} onCheckedChange={(value) => void setSetting("menuBarEnabled", String(value))} label={t("settings.menuBar")} /></div>
          <div className="setting-row multiline">
            <div><strong>{t("settings.repairHooks")}</strong><p>{t("settings.repairHooksBody")}</p></div>
            <button className="button secondary" disabled={repairHooks.isPending} onClick={() => repairHooks.mutate()}>
              <RefreshCw size={13} />{repairHooks.isPending ? t("actions.refreshing") : t("live.repair")}
            </button>
          </div>
          <p className="setting-callout live-setting-status"><PanelTop size={13} />{t(`settings.liveState.${live.data?.hookStatus.state ?? "unavailable"}`)} · {t("settings.rawRetention")}</p>
        </section>

        <section className="settings-section">
          <header><LockKeyhole size={17} /><div><h2>{t("settings.access")}</h2></div></header>
          <div className="setting-row multiline"><div><strong>{t("settings.vctiStructure")}</strong><p><ScanSearch size={12} />{t("settings.vctiStructureBody")}</p></div><Toggle checked={data.vctiPromptStructure === "true"} onCheckedChange={(value) => void setSetting("vctiPromptStructure", String(value))} label={t("settings.vctiStructure")} /></div>
          <div className="setting-row multiline"><div><strong>{t("settings.gitRead")}</strong><p>{t("settings.gitReadBody")}</p></div><div className="setting-toggle-with-status">{historyRefreshPending ? <span className="setting-progress" role="status" aria-live="polite"><LoaderCircle className="spin" size={13} />{t("actions.refreshing")}</span> : null}<Toggle checked={data.gitReadAllowed === "true"} disabled={historyRefreshPending} onCheckedChange={(value) => void setSetting("gitReadAllowed", String(value))} label={t("settings.gitRead")} /></div></div>
          <div className="setting-row multiline"><div><strong>{t("settings.credentials")}</strong><p>{t("settings.credentialsBody")}</p></div><Toggle checked={data.credentialsAllowed === "true"} disabled={cursorRefreshPending} onCheckedChange={(value) => void setSetting("credentialsAllowed", String(value))} label={t("settings.credentials")} /></div>
          <div className={`setting-row multiline nested-setting ${data.credentialsAllowed !== "true" ? "disabled-setting" : ""}`}>
            <div><strong>{t("settings.cursorDashboardUsage")}</strong><p>{t(data.credentialsAllowed === "true" ? "settings.cursorDashboardUsageBody" : "settings.cursorDashboardUsageRequiresCredentials")}</p></div>
            <div className="setting-toggle-with-status">
              {cursorRefreshPending ? <span className="setting-progress" role="status" aria-live="polite"><LoaderCircle className="spin" size={13} />{t("cursorUsage.loadingShort")}</span> : null}
              <Toggle checked={cursorDashboardDraft ?? data.cursorDashboardUsage === "true"} disabled={data.credentialsAllowed !== "true" || cursorRefreshPending} onCheckedChange={setCursorDashboard} label={t("settings.cursorDashboardUsage")} />
            </div>
          </div>
          <div className="setting-row multiline">
            <div><strong>{t("settings.useSystemProxy")}</strong><p>{t("settings.useSystemProxyBody")}</p></div>
            <Toggle checked={data.useSystemProxy === "true"} disabled={cursorRefreshPending} onCheckedChange={(value) => void setSetting("useSystemProxy", String(value))} label={t("settings.useSystemProxy")} />
          </div>
        </section>

        <section className="settings-section">
          <header><HardDrive size={17} /><div><h2>{t("settings.retention")}</h2><p>{t("settings.retentionBody")}</p></div></header>
          <div className="setting-row"><div><strong>{t("settings.retention")}</strong></div><select value={data.retentionDays} onChange={(event) => void setSetting("retentionDays", event.target.value)}>{[30, 90, 180, 365, 730].map((days) => <option key={days} value={days}>{t("settings.days", { count: days })}</option>)}</select></div>
          <DiagnosticRetentionControl
            status={diagnosticStatus}
            locale={locale}
            pending={diagnosticPending}
            loading={diagnostics.isLoading}
            hasError={diagnostics.isError
              || setDiagnostics.isError
              || clearDiagnostics.isError
              || diagnosticStatus?.state === "unavailable"}
            clearCount={diagnosticClearCount}
            onToggle={toggleDiagnostics}
            onClear={() => { if (window.confirm(t("settings.diagnosticClearConfirm"))) clearDiagnostics.mutate(); }}
          />
        </section>

        <section className="settings-section project-settings">
          <header><Database size={17} /><div><h2>{t("settings.projects")}</h2><p>{t("settings.projectsBody")}</p></div></header>
          <div className="project-list">{projects.data?.map((project) => <div key={project.projectHash}><span className="project-glyph"><GitBranch size={15} /></span><span><strong>{project.projectLabel}</strong><small>{t("settings.projectSessions", { count: project.sessionCount })}</small></span><button className={project.excluded ? "button secondary" : "button danger-button"} onClick={() => { if (project.excluded || window.confirm(t("settings.excludeConfirm"))) exclude.mutate({ hash: project.projectHash, excluded: project.excluded }); }}>{project.excluded ? t("settings.include") : t("settings.exclude")}</button></div>)}</div>
        </section>

        <section className="settings-section">
          <header><Power size={17} /><div><h2>{t("settings.startup")}</h2></div></header>
          <div className="setting-row multiline"><div><strong>{t("settings.launchAtLogin")}</strong><p>{t("settings.launchBody")}</p></div><Toggle checked={loginEnabled} onCheckedChange={(value) => void setLogin(value)} label={t("settings.launchAtLogin")} /></div>
        </section>

        <section className="settings-section danger-zone">
          <header><ShieldAlert size={17} /><div><h2>{t("settings.localData")}</h2><p>{t("settings.clearDataBody")}</p></div></header>
          <button className="button danger-button" disabled={clearData.isPending} onClick={() => { if (window.confirm(t("settings.clearConfirm"))) clearData.mutate(); }}><Trash2 size={14} />{t("settings.clearData")}</button>
          <small>{t("settings.version", { version: appVersion })}</small>
        </section>
      </div>
    </div>
  );
}
