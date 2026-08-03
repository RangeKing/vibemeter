import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { ArrowRight, Database, GitBranch, HardDrive, Languages, Laptop, LockKeyhole, PanelTop, Power, RadioTower, RefreshCw, ScanSearch, ShieldAlert, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import desktopPackage from "../../package.json";
import { CursorAccountUsagePanel } from "../components/CursorAccountUsagePanel";
import { ErrorState, LoadingState, PageHeader, Toggle } from "../components/ui";
import { api } from "../lib/api";
import { useUiStore } from "../store";
import type { AppSettings, Locale, Theme } from "../types";

export function SettingsPage({ locale }: { locale: Locale }) {
  const { t } = useTranslation();
  const appVersion = desktopPackage.version;
  const client = useQueryClient();
  const setPage = useUiStore((state) => state.setPage);
  const range = useUiStore((state) => state.range);
  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  const projects = useQuery({ queryKey: ["projects"], queryFn: api.projects });
  const live = useQuery({ queryKey: ["live-snapshot"], queryFn: api.liveSnapshot, refetchInterval: 3_000 });
  const [loginEnabled, setLoginEnabled] = useState(false);
  useEffect(() => { void isEnabled().then(setLoginEnabled).catch(() => setLoginEnabled(false)); }, []);

  const setSetting = async (key: keyof AppSettings, value: string) => {
    await api.setSetting(key, value);
    if (key === "credentialsAllowed" && value === "false") {
      await api.setSetting("cursorDashboardUsage", "false");
    }
    if (key === "credentialsAllowed" || key === "cursorDashboardUsage") {
      const credentialsAllowed = key === "credentialsAllowed"
        ? value === "true"
        : settings.data?.credentialsAllowed === "true";
      const cursorDashboardUsageEnabled = key === "cursorDashboardUsage"
        ? value === "true"
        : credentialsAllowed && settings.data?.cursorDashboardUsage === "true";
      await api.refreshProviders(credentialsAllowed, cursorDashboardUsageEnabled);
    }
    if (key === "gitReadAllowed" || key === "vctiPromptStructure") await api.refreshIndex(true);
    await Promise.all([
      client.invalidateQueries({ queryKey: ["settings"] }),
      client.invalidateQueries({ queryKey: ["providers"] }),
      client.invalidateQueries({ queryKey: ["menu-snapshot"] }),
      client.invalidateQueries({ queryKey: ["vcti"] }),
      client.invalidateQueries({ queryKey: ["overview"] }),
      client.invalidateQueries({ queryKey: ["insights"] }),
    ]);
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

  if (settings.isLoading || projects.isLoading) return <LoadingState />;
  if (settings.isError || !settings.data || projects.isError) return <ErrorState retry={() => void Promise.all([settings.refetch(), projects.refetch()])} />;
  const data = settings.data;
  const theme = data.theme as Theme;
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
          <header><RadioTower size={17} /><div><h2>{t("settings.live")}</h2><p>{t("settings.liveBody")}</p></div></header>
          <div className="setting-row multiline"><div><strong>{t("settings.liveHooks")}</strong><p>{t("settings.liveHooksBody")}</p></div><Toggle checked={data.liveHooksEnabled === "true"} onCheckedChange={(value) => void setSetting("liveHooksEnabled", String(value))} label={t("settings.liveHooks")} /></div>
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
          <div className="setting-row multiline"><div><strong>{t("settings.gitRead")}</strong><p>{t("settings.gitReadBody")}</p></div><Toggle checked={data.gitReadAllowed === "true"} onCheckedChange={(value) => void setSetting("gitReadAllowed", String(value))} label={t("settings.gitRead")} /></div>
          <div className="setting-row multiline"><div><strong>{t("settings.credentials")}</strong><p>{t("settings.credentialsBody")}</p></div><Toggle checked={data.credentialsAllowed === "true"} onCheckedChange={(value) => void setSetting("credentialsAllowed", String(value))} label={t("settings.credentials")} /></div>
          <div className={`setting-row multiline nested-setting ${data.credentialsAllowed !== "true" ? "disabled-setting" : ""}`}><div><strong>{t("settings.cursorDashboardUsage")}</strong><p>{t(data.credentialsAllowed === "true" ? "settings.cursorDashboardUsageBody" : "settings.cursorDashboardUsageRequiresCredentials")}</p></div><Toggle checked={data.cursorDashboardUsage === "true"} disabled={data.credentialsAllowed !== "true"} onCheckedChange={(value) => void setSetting("cursorDashboardUsage", String(value))} label={t("settings.cursorDashboardUsage")} /></div>
          {data.cursorDashboardUsage === "true" && data.credentialsAllowed === "true" ? (
            <div className="settings-cursor-usage">
              <CursorAccountUsagePanel locale={locale} range={range} />
            </div>
          ) : null}
        </section>

        <section className="settings-section">
          <header><HardDrive size={17} /><div><h2>{t("settings.retention")}</h2><p>{t("settings.retentionBody")}</p></div></header>
          <div className="setting-row"><div><strong>{t("settings.retention")}</strong></div><select value={data.retentionDays} onChange={(event) => void setSetting("retentionDays", event.target.value)}>{[30, 90, 180, 365, 730].map((days) => <option key={days} value={days}>{t("settings.days", { count: days })}</option>)}</select></div>
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
