import { useQuery, useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { AppShell } from "./components/AppShell";
import { MenuBarPopover } from "./components/MenuBarPopover";
import { NotchSurface } from "./components/NotchSurface";
import { Onboarding } from "./components/Onboarding";
import { LoadingState } from "./components/ui";
import { api } from "./lib/api";
import { DataPage } from "./pages/DataPage";
import { InsightsPage } from "./pages/InsightsPage";
import { LivePage } from "./pages/LivePage";
import { SessionsPage } from "./pages/SessionsPage";
import { SettingsPage } from "./pages/SettingsPage";
import { ShareStudioPage } from "./pages/ShareStudioPage";
import { SourcesPage } from "./pages/SourcesPage";
import { VctiPage } from "./pages/VctiPage";
import { useUiStore } from "./store";
import type { Locale, PageKey } from "./types";

function systemLocale(): Locale {
  return navigator.language.toLowerCase().startsWith("zh") ? "zh-CN" : "en-US";
}

const pages: PageKey[] = ["data", "live", "insights", "sessions", "vcti", "share", "sources", "settings"];

export function App({ surface }: { surface: "main" | "menubar" | "notch" }) {
  const { i18n } = useTranslation();
  const client = useQueryClient();
  const page = useUiStore((state) => state.page);
  const setPage = useUiStore((state) => state.setPage);
  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  const index = useQuery({ queryKey: ["index-status"], queryFn: api.indexStatus, refetchInterval: 1_500 });
  const locale: Locale = settings.data?.locale === "zh-CN" || settings.data?.locale === "en-US" ? settings.data.locale : systemLocale();

  useEffect(() => {
    void i18n.changeLanguage(locale);
    document.documentElement.lang = locale;
  }, [i18n, locale]);

  useEffect(() => {
    document.documentElement.dataset.theme = settings.data?.theme ?? "system";
  }, [settings.data?.theme]);

  useEffect(() => {
    if (surface !== "main" || !index.data?.finishedAt) return;
    void Promise.all([
      client.invalidateQueries({ queryKey: ["sources"] }),
      client.invalidateQueries({ queryKey: ["overview"] }),
      client.invalidateQueries({ queryKey: ["tasks"] }),
      client.invalidateQueries({ queryKey: ["sessions"] }),
      client.invalidateQueries({ queryKey: ["session"] }),
      client.invalidateQueries({ queryKey: ["share-sessions"] }),
    ]);
  }, [client, index.data?.finishedAt, surface]);

  useEffect(() => {
    if (surface !== "main") return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<string>("navigate", (event) => {
      const legacyMap: Record<string, PageKey> = { overview: "data", today: "data", compare: "insights", playbook: "insights", reviews: "insights" };
      const target = legacyMap[event.payload] ?? event.payload;
      if (pages.includes(target as PageKey)) setPage(target as PageKey);
    }).then((cleanup) => { if (disposed) cleanup(); else unlisten = cleanup; });
    return () => { disposed = true; unlisten?.(); };
  }, [setPage, surface]);

  if (settings.isLoading) return <LoadingState />;
  if (surface === "menubar") return <MenuBarPopover locale={locale} />;
  if (surface === "notch") return <NotchSurface locale={locale} />;
  if (settings.data?.onboardingComplete !== "true") {
    return <Onboarding onFinish={async ({ credentialsAllowed, gitReadAllowed, vctiPromptStructure }) => {
      await Promise.all([
        api.setSetting("onboardingComplete", "true"),
        api.setSetting("credentialsAllowed", String(credentialsAllowed)),
        api.setSetting("gitReadAllowed", String(gitReadAllowed)),
        api.setSetting("vctiPromptStructure", String(vctiPromptStructure)),
        api.setSetting("liveHooksEnabled", "true"),
        api.setSetting("notchEnabled", "true"),
        api.setSetting("menuBarEnabled", "true"),
      ]);
      if (credentialsAllowed) await api.refreshProviders(true, false);
      await api.refreshIndex(true);
      await client.invalidateQueries({ queryKey: ["settings"] });
    }} />;
  }

  const content = (() => {
    switch (page) {
      case "live": return <LivePage locale={locale} />;
      case "data": return <DataPage locale={locale} />;
      case "sessions": return <SessionsPage locale={locale} />;
      case "insights": return <InsightsPage locale={locale} />;
      case "vcti": return <VctiPage locale={locale} />;
      case "share": return <ShareStudioPage locale={locale} />;
      case "sources": return <SourcesPage locale={locale} />;
      case "settings": return <SettingsPage />;
    }
  })();
  return <AppShell indexStatus={index.data}>{content}</AppShell>;
}
