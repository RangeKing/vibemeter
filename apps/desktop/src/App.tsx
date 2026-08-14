import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { AppShell } from "./components/AppShell";
import { MenuBarPopover } from "./components/MenuBarPopover";
import { NotchSurface } from "./components/NotchSurface";
import { Onboarding } from "./components/Onboarding";
import { LoadingState } from "./components/ui";
import { SessionsWorkspace } from "./components/SessionsWorkspace";
import { api } from "./lib/api";
import { DataPage } from "./pages/DataPage";
import { LivePage } from "./pages/LivePage";
import { SettingsPage } from "./pages/SettingsPage";
import { ShareStudioPage } from "./pages/ShareStudioPage";
import { SourcesPage } from "./pages/SourcesPage";
import { VctiPage } from "./pages/VctiPage";
import { useUiStore } from "./store";
import type { Locale, PageKey } from "./types";

function systemLocale(): Locale {
  return navigator.language.toLowerCase().startsWith("zh") ? "zh-CN" : "en-US";
}

const pages: PageKey[] = ["data", "sessions", "live", "vcti", "share", "sources", "settings"];

export function App({ surface }: { surface: "main" | "menubar" | "notch" }) {
  const { i18n } = useTranslation();
  const client = useQueryClient();
  const page = useUiStore((state) => state.page);
  const setPage = useUiStore((state) => state.setPage);
  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  const index = useQuery({ queryKey: ["index-status"], queryFn: api.indexStatus, refetchInterval: 1_500 });
  const locale: Locale = settings.data?.locale === "zh-CN" || settings.data?.locale === "en-US" ? settings.data.locale : systemLocale();
  const dismissMigration = useMutation({
    mutationFn: () => api.setSetting("iaMigrationTipSeen", "true"),
    onSuccess: async () => {
      await client.invalidateQueries({ queryKey: ["settings"] });
    },
  });

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
      const legacyMap: Record<string, PageKey> = {
        overview: "data",
        today: "data",
        compare: "vcti",
        playbook: "vcti",
        reviews: "vcti",
        insights: "vcti",
        sessions: "sessions",
      };
      const mapped = legacyMap[event.payload] ?? event.payload;
      if (pages.includes(mapped as PageKey)) setPage(mapped as PageKey);
    }).then((cleanup) => { if (disposed) cleanup(); else unlisten = cleanup; });
    return () => { disposed = true; unlisten?.(); };
  }, [setPage, surface]);

  useEffect(() => {
    if (page === "insights") setPage("vcti");
  }, [page, setPage]);

  if (settings.isLoading) return <LoadingState />;
  if (surface === "menubar") return <MenuBarPopover locale={locale} />;
  if (surface === "notch") return <NotchSurface locale={locale} />;
  if (settings.data?.onboardingComplete !== "true") {
    return <Onboarding onComplete={async () => {
      await client.invalidateQueries({ queryKey: ["settings"] });
      await client.invalidateQueries({ queryKey: ["sources"] });
      await client.invalidateQueries({ queryKey: ["vcti"] });
      await client.invalidateQueries({ queryKey: ["index-status"] });
      setPage("vcti");
    }} />;
  }

  const content = (() => {
    switch (page) {
      case "live": return <LivePage locale={locale} />;
      case "data": return <DataPage locale={locale} />;
      case "sessions": return <SessionsWorkspace locale={locale} />;
      case "insights": return <VctiPage locale={locale} />;
      case "vcti": return <VctiPage locale={locale} />;
      case "share": return <ShareStudioPage locale={locale} />;
      case "sources": return <SourcesPage locale={locale} />;
      case "settings": return <SettingsPage locale={locale} />;
    }
  })();
  const showMigrationTip = settings.data?.iaMigrationTipSeen !== "true";
  return (
    <AppShell
      indexStatus={index.data}
      showMigrationTip={showMigrationTip}
      onDismissMigrationTip={() => dismissMigration.mutate()}
    >
      {content}
    </AppShell>
  );
}
