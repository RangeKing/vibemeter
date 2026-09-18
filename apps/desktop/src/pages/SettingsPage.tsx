import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { ArrowRight, BarChart3, Database, GitBranch, HardDrive, KeyRound, Languages, Laptop, LoaderCircle, LockKeyhole, PanelTop, Power, RadioTower, RefreshCw, ScanSearch, ShieldAlert, Trash2, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { FormEvent as ReactFormEvent, PointerEvent as ReactPointerEvent } from "react";
import { useTranslation } from "react-i18next";
import desktopPackage from "../../package.json";
import { AgentBadge, ErrorState, LoadingState, PageHeader, Toggle } from "../components/ui";
import { api } from "../lib/api";
import { refreshHistoryIndex } from "../lib/indexRefresh";
import { detectedDataAgents, parseDataPageAgents, serializeDataPageAgents, sourceCapabilityNameGroups } from "../lib/sourceStatus";
import { agentName } from "../lib/format";
import { agentSubscription, isApiProvider, parseEdgeAgents, providerDisplayName, serializeEdgeAgents } from "../lib/quota";
import { useFocusTrap } from "../lib/useFocusTrap";
import { useUiStore } from "../store";
import type { AppSettings, DiagnosticRetentionStatus, Locale, ProjectControl, Theme } from "../types";

type ProjectGroupDialog = { mode: "create" } | { mode: "rename"; groupId: string };

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
  const apiAccounts = useQuery({ queryKey: ["api-accounts"], queryFn: api.apiAccounts });
  const [apiDraft, setApiDraft] = useState({ provider: "deepseek", label: "", key: "" });
  const [apiPending, setApiPending] = useState(false);
  const [apiError, setApiError] = useState(false);
  const [edgeAgentsDraft, setEdgeAgentsDraft] = useState<string | null>(null);
  const [edgeAgentsPending, setEdgeAgentsPending] = useState(false);
  const [dataPageAgentsDraft, setDataPageAgentsDraft] = useState<string | null>(null);
  const [dataPageAgentsPending, setDataPageAgentsPending] = useState(false);
  const [agentDetectionPending, setAgentDetectionPending] = useState(false);
  const [diagnosticClearCount, setDiagnosticClearCount] = useState<number | null>(null);
  const [selectedProjectHashes, setSelectedProjectHashes] = useState<string[]>([]);
  const projectSelectionAnchor = useRef<string | undefined>(undefined);
  const projectSelectionModifiers = useRef({ shiftKey: false, metaKey: false, ctrlKey: false });
  const [projectGroupDialog, setProjectGroupDialog] = useState<ProjectGroupDialog>();
  const [projectGroupName, setProjectGroupName] = useState("");
  const projectGroupDialogRef = useFocusTrap(Boolean(projectGroupDialog));
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
  const invalidateProjectQueries = async () => {
    await Promise.all([
      client.invalidateQueries({ queryKey: ["projects"] }),
      client.invalidateQueries({ queryKey: ["sessions"] }),
      client.invalidateQueries({ queryKey: ["project-summaries"] }),
    ]);
  };
  const createProjectGroup = useMutation({
    mutationFn: ({ name, hashes }: { name: string; hashes: string[] }) => api.createProjectGroup(name, hashes),
    onSuccess: async () => {
      setSelectedProjectHashes([]);
      setProjectGroupDialog(undefined);
      setProjectGroupName("");
      await invalidateProjectQueries();
    },
  });
  const renameProjectGroup = useMutation({
    mutationFn: ({ id, name }: { id: string; name: string }) => api.renameProjectGroup(id, name),
    onSuccess: async () => {
      setProjectGroupDialog(undefined);
      setProjectGroupName("");
      await invalidateProjectQueries();
    },
  });
  const updateProjectGroupMembers = useMutation({
    mutationFn: ({ id, hashes }: { id: string; hashes: string[] }) => api.updateProjectGroupMembers(id, hashes),
    onSuccess: invalidateProjectQueries,
  });
  const deleteProjectGroup = useMutation({
    mutationFn: api.deleteProjectGroup,
    onSuccess: invalidateProjectQueries,
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
  useEffect(() => {
    if (!projectGroupDialog) return;
    document.body.classList.add("modal-open");
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || createProjectGroup.isPending || renameProjectGroup.isPending) return;
      setProjectGroupDialog(undefined);
      setProjectGroupName("");
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      document.body.classList.remove("modal-open");
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, [createProjectGroup.isPending, projectGroupDialog, renameProjectGroup.isPending]);

  const projectGroups = useMemo(() => {
    const groups = new Map<string, { id: string; name: string; members: ProjectControl[] }>();
    for (const project of projects.data ?? []) {
      if (!project.groupId || !project.groupName) continue;
      const current = groups.get(project.groupId) ?? { id: project.groupId, name: project.groupName, members: [] };
      current.members.push(project);
      groups.set(project.groupId, current);
    }
    return [...groups.values()].sort((left, right) => left.name.localeCompare(right.name));
  }, [projects.data]);

  if (settings.isLoading || projects.isLoading || sources.isLoading) return <LoadingState />;
  if (settings.isError || !settings.data || projects.isError || sources.isError || !sources.data) return <ErrorState retry={() => void Promise.all([settings.refetch(), projects.refetch(), sources.refetch()])} />;
  const data = settings.data;
  const ungroupedProjects = projects.data?.filter((project) => !project.groupId && !project.excluded) ?? [];
  const detectedAgents = detectedDataAgents(sources.data);
  const configuredDataAgents = parseDataPageAgents(dataPageAgentsDraft ?? data.dataPageAgents);
  const autoDataPageAgents = configuredDataAgents === undefined;
  const selectedDataAgents = new Set(configuredDataAgents ?? detectedAgents);
  const theme = data.theme as Theme;
  const diagnosticStatus = diagnostics.data;
  const diagnosticPending = setDiagnostics.isPending || clearDiagnostics.isPending;
  const configuredEdgeAgents = parseEdgeAgents(edgeAgentsDraft ?? data.edgeSidebarAgents);
  /* Mirrors the sidebar's automatic list, so the ticks show what is actually
     on screen rather than what would be if every Agent had a subscription. */
  const autoEdgeAgents = detectedAgents.filter((agent) => agentSubscription(agent) !== undefined);
  const selectedEdgeAgents = new Set(configuredEdgeAgents ?? autoEdgeAgents);
  const toggleEdgeAgent = async (agent: string, checked: boolean) => {
    const base = configuredEdgeAgents ?? autoEdgeAgents;
    const next = checked ? [...base, agent] : base.filter((item) => item !== agent);
    const previous = edgeAgentsDraft;
    const value = serializeEdgeAgents(next);
    setEdgeAgentsDraft(value);
    setEdgeAgentsPending(true);
    try {
      await setSetting("edgeSidebarAgents", value);
    } catch {
      setEdgeAgentsDraft(previous);
    } finally {
      setEdgeAgentsPending(false);
    }
  };
  const apiProviders = ["deepseek", "moonshot"];
  const addApiAccount = async () => {
    setApiPending(true);
    setApiError(false);
    try {
      await api.addApiAccount(apiDraft.provider, apiDraft.label, apiDraft.key);
      // The key is gone from the page the moment it is stored; it lives in the
      // keychain and is never read back here.
      setApiDraft({ provider: apiDraft.provider, label: "", key: "" });
      await apiAccounts.refetch();
    } catch {
      setApiError(true);
    } finally {
      setApiPending(false);
    }
  };
  const removeApiAccount = async (id: string) => {
    setApiPending(true);
    setApiError(false);
    try {
      await api.removeApiAccount(id);
      await apiAccounts.refetch();
    } catch {
      setApiError(true);
    } finally {
      setApiPending(false);
    }
  };
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
  const captureProjectSelectionModifiers = (event: ReactPointerEvent<HTMLLabelElement>) => {
    if ((event.target as HTMLElement).closest("button")) {
      projectSelectionModifiers.current = { shiftKey: false, metaKey: false, ctrlKey: false };
      return;
    }
    projectSelectionModifiers.current = { shiftKey: event.shiftKey, metaKey: event.metaKey, ctrlKey: event.ctrlKey };
  };
  const toggleProjectSelection = (projectHash: string, checked: boolean) => {
    const modifiers = projectSelectionModifiers.current;
    projectSelectionModifiers.current = { shiftKey: false, metaKey: false, ctrlKey: false };
    const orderedHashes = ungroupedProjects.map((project) => project.projectHash);
    const projectIndex = orderedHashes.indexOf(projectHash);
    const anchorIndex = projectSelectionAnchor.current ? orderedHashes.indexOf(projectSelectionAnchor.current) : -1;
    const hasRange = modifiers.shiftKey && anchorIndex >= 0 && projectIndex >= 0;

    if (hasRange) {
      const start = Math.min(anchorIndex, projectIndex);
      const end = Math.max(anchorIndex, projectIndex);
      const rangeHashes = orderedHashes.slice(start, end + 1);
      setSelectedProjectHashes((current) => {
        const next = new Set(current);
        rangeHashes.forEach((hash) => {
          if (checked) next.add(hash);
          else next.delete(hash);
        });
        return orderedHashes.filter((hash) => next.has(hash));
      });
    } else {
      setSelectedProjectHashes((current) => checked
        ? [...new Set([...current, projectHash])]
        : current.filter((hash) => hash !== projectHash));
    }

    if (!hasRange) projectSelectionAnchor.current = projectHash;
  };
  const openCreateProjectGroup = () => {
    if (selectedProjectHashes.length < 2) return;
    setProjectGroupName("");
    setProjectGroupDialog({ mode: "create" });
  };
  const openRenameProjectGroup = (group: { id: string; name: string }) => {
    setProjectGroupName(group.name);
    setProjectGroupDialog({ mode: "rename", groupId: group.id });
  };
  const closeProjectGroupDialog = () => {
    if (createProjectGroup.isPending || renameProjectGroup.isPending) return;
    setProjectGroupDialog(undefined);
    setProjectGroupName("");
  };
  const submitProjectGroup = (event: ReactFormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const dialog = projectGroupDialog;
    const name = projectGroupName.trim();
    if (!dialog || !name) return;
    if (dialog.mode === "create") {
      createProjectGroup.mutate({ name, hashes: selectedProjectHashes });
    } else {
      renameProjectGroup.mutate({ id: dialog.groupId, name });
    }
  };
  const updateGroupMembers = (group: { id: string; members: ProjectControl[] }, projectHash: string, checked: boolean) => {
    const current = group.members.map((member) => member.projectHash);
    const next = checked ? [...new Set([...current, projectHash])] : current.filter((hash) => hash !== projectHash);
    updateProjectGroupMembers.mutate({ id: group.id, hashes: next });
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
          <header><PanelTop size={17} /><div><h2>{t("edge.title")}</h2><p>{t("edge.description")}</p></div></header>
          <div className="setting-row"><div><strong>{t("edge.enabled")}</strong></div><Toggle checked={data.edgeSidebarEnabled === "true"} onCheckedChange={(enabled) => void setSetting("edgeSidebarEnabled", String(enabled))} label={t("edge.enabled")} /></div>
          <div className="setting-row"><div><strong>{t("edge.position")}</strong></div><select aria-label={t("edge.position")} value={data.edgeSidebarSide ?? "right"} onChange={(event) => void setSetting("edgeSidebarSide", event.target.value)}><option value="left">{t("edge.left")}</option><option value="right">{t("edge.right")}</option></select></div>
          <div className="setting-row multiline"><div><strong>{t("edge.agents")}</strong><p>{t("edge.agentsBody")}</p></div></div>
          <div className="data-page-agent-options" role="group" aria-label={t("edge.agents")}>
            {sources.data.map((source) => (
              <label className={`data-page-agent-option ${source.available ? "" : "is-missing"}`} key={source.agent}>
                <AgentBadge agent={source.agent} compact />
                <span><strong>{source.available ? t("settings.dataPageAgentsDetected") : t("settings.dataPageAgentsNotDetected")}</strong><small>{agentSubscription(source.agent) ? t("edge.agentHasQuota") : t("edge.agentNoQuota")}</small></span>
                <input
                  type="checkbox"
                  checked={selectedEdgeAgents.has(source.agent)}
                  disabled={edgeAgentsPending}
                  aria-label={t("edge.agentToggle", { agent: agentName(source.agent) })}
                  onChange={(event) => void toggleEdgeAgent(source.agent, event.target.checked)}
                />
              </label>
            ))}
          </div>
        </section>

        <section className="settings-section">
          <header><KeyRound size={17} /><div><h2>{t("edge.accounts")}</h2><p>{t("edge.accountsBody")}</p></div></header>
          <div className="api-account-list">
            {apiAccounts.data?.length ? apiAccounts.data.map((account) => (
              <div className="api-account-row" key={account.id}>
                <AgentBadge agent={account.provider === "moonshot" ? "kimi-code" : "deepseek-harness"} compact />
                <span><strong>{account.label}</strong><small>{providerDisplayName(account.provider)}</small></span>
                <button className="button subtle" disabled={apiPending} aria-label={t("edge.accountRemove", { label: account.label })} onClick={() => void removeApiAccount(account.id)}><Trash2 size={13} /></button>
              </div>
            )) : <p className="api-account-empty">{t("edge.accountEmpty")}</p>}
          </div>
          <div className="api-account-form">
            <select aria-label={t("settings.sources")} value={apiDraft.provider} onChange={(event) => setApiDraft({ ...apiDraft, provider: event.target.value })}>
              {apiProviders.filter(isApiProvider).map((provider) => <option key={provider} value={provider}>{providerDisplayName(provider)}</option>)}
            </select>
            <input aria-label={t("edge.accountLabel")} placeholder={t("edge.accountLabel")} value={apiDraft.label} onChange={(event) => setApiDraft({ ...apiDraft, label: event.target.value })} />
            <input aria-label={t("edge.accountKey")} placeholder={t("edge.accountKey")} type="password" autoComplete="off" spellCheck={false} value={apiDraft.key} onChange={(event) => setApiDraft({ ...apiDraft, key: event.target.value })} />
            <button className="button primary" disabled={apiPending || !apiDraft.key.trim()} onClick={() => void addApiAccount()}>{t("edge.accountAdd")}</button>
          </div>
          <p className="api-account-hint">{t("edge.accountKeyHint")}</p>
          {apiError ? <p className="api-account-hint danger" role="alert">{t("edge.error")}</p> : null}
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
                  disabled={!source.available || dataPageAgentsPending}
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
          <div className="project-group-controls">
            <p>{t("settings.projectGroupsBody")}</p>
            <button className="button secondary" disabled={selectedProjectHashes.length < 2 || createProjectGroup.isPending} onClick={openCreateProjectGroup}>{t("settings.createProjectGroup", { count: selectedProjectHashes.length })}</button>
          </div>
          <div className="project-group-list">
            {projectGroups.map((group) => {
              const eligible = projects.data?.filter((project) => !project.excluded && (!project.groupId || project.groupId === group.id)) ?? [];
              return <article className="project-group-card" key={group.id}>
                <header><span className="project-glyph"><GitBranch size={15} /></span><span><strong>{group.name}</strong><small>{t("settings.projectGroupMembers", { count: group.members.length })}</small></span><button className="button secondary" onClick={() => openRenameProjectGroup(group)}>{t("actions.edit")}</button><button className="button danger-button" onClick={() => { if (window.confirm(t("settings.removeProjectGroupConfirm"))) deleteProjectGroup.mutate(group.id); }}>{t("settings.removeProjectGroup")}</button></header>
                <div className="project-group-member-list">{eligible.map((project) => <label key={project.projectHash}><input type="checkbox" checked={project.groupId === group.id} onChange={(event) => updateGroupMembers(group, project.projectHash, event.target.checked)} /><span><strong>{project.projectLabel}</strong><small>{t("settings.projectSessions", { count: project.sessionCount })} · {project.localPath ?? t("metrics.notRecorded")}</small></span><button className={project.excluded ? "button secondary" : "button danger-button"} onClick={(event) => { event.preventDefault(); if (project.excluded || window.confirm(t("settings.excludeConfirm"))) exclude.mutate({ hash: project.projectHash, excluded: project.excluded }); }}>{project.excluded ? t("settings.include") : t("settings.exclude")}</button></label>)}</div>
              </article>;
            })}
            <div className="project-list">{ungroupedProjects.map((project) => <label className="project-list-row" key={project.projectHash} onPointerDown={captureProjectSelectionModifiers}><input type="checkbox" checked={selectedProjectHashes.includes(project.projectHash)} aria-label={project.projectLabel} onKeyDown={(event) => { if (event.key === " " || event.key === "Enter") projectSelectionModifiers.current = { shiftKey: event.shiftKey, metaKey: event.metaKey, ctrlKey: event.ctrlKey }; }} onChange={(event) => toggleProjectSelection(project.projectHash, event.target.checked)} /><span className="project-glyph"><GitBranch size={15} /></span><span><strong>{project.projectLabel}</strong><small>{t("settings.projectSessions", { count: project.sessionCount })} · {project.localPath ?? t("metrics.notRecorded")}</small></span><button className="button danger-button" onClick={(event) => { event.preventDefault(); if (window.confirm(t("settings.excludeConfirm"))) exclude.mutate({ hash: project.projectHash, excluded: false }); }}>{t("settings.exclude")}</button></label>)}</div>
            {projects.data?.filter((project) => project.excluded).map((project) => <div className="project-list-row excluded-project-row" key={project.projectHash}><span className="project-glyph"><GitBranch size={15} /></span><span><strong>{project.projectLabel}</strong><small>{t("settings.projectSessions", { count: project.sessionCount })} · {project.localPath ?? t("metrics.notRecorded")}</small></span><button className="button secondary" onClick={() => exclude.mutate({ hash: project.projectHash, excluded: true })}>{t("settings.include")}</button></div>)}
          </div>
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

        {projectGroupDialog ? (
          <div
            className="modal-backdrop project-group-dialog-backdrop"
            role="presentation"
            onMouseDown={(event) => {
              if (event.target === event.currentTarget) closeProjectGroupDialog();
            }}
          >
            <section
              ref={projectGroupDialogRef}
              className="project-group-dialog"
              role="dialog"
              aria-modal="true"
              aria-labelledby="project-group-dialog-title"
            >
              <header>
                <div>
                  <span className="project-group-dialog-kicker">{t("settings.projects")}</span>
                  <h2 id="project-group-dialog-title">
                    {t(projectGroupDialog.mode === "create" ? "settings.projectGroupNamePrompt" : "settings.projectGroupRenamePrompt")}
                  </h2>
                </div>
                <button className="icon-button" type="button" aria-label={t("actions.close")} onClick={closeProjectGroupDialog} disabled={createProjectGroup.isPending || renameProjectGroup.isPending}>
                  <X size={16} />
                </button>
              </header>
              <form onSubmit={submitProjectGroup}>
                <label htmlFor="project-group-name">{t(projectGroupDialog.mode === "create" ? "settings.projectGroupNamePrompt" : "settings.projectGroupRenamePrompt")}</label>
                <input
                  id="project-group-name"
                  value={projectGroupName}
                  onChange={(event) => setProjectGroupName(event.target.value)}
                  disabled={createProjectGroup.isPending || renameProjectGroup.isPending}
                  required
                />
                <footer>
                  <button className="button secondary" type="button" onClick={closeProjectGroupDialog} disabled={createProjectGroup.isPending || renameProjectGroup.isPending}>{t("actions.cancel")}</button>
                  <button className="button primary" type="submit" disabled={!projectGroupName.trim() || createProjectGroup.isPending || renameProjectGroup.isPending}>{t("actions.save")}</button>
                </footer>
              </form>
            </section>
          </div>
        ) : null}
      </div>
    </div>
  );
}
