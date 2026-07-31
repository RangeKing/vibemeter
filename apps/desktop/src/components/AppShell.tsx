import {
  BarChart3,
  ScanFace,
  Settings,
  Share2,
  Sparkles,
  RadioTower,
} from "lucide-react";
import { useEffect, useRef, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import appIconUrl from "../../src-tauri/icons/vibemeter-icon-source.png";
import { useUiStore } from "../store";
import type { IndexStatus, PageKey } from "../types";
import { IaMigrationTip } from "./IaMigrationTip";

const primary: Array<{ id: PageKey; icon: typeof Sparkles }> = [
  { id: "data", icon: BarChart3 },
  { id: "live", icon: RadioTower },
  { id: "vcti", icon: ScanFace },
];

const utility: Array<{ id: PageKey; icon: typeof Sparkles }> = [
  { id: "share", icon: Share2 },
  { id: "settings", icon: Settings },
];

export function AppShell({
  children,
  indexStatus,
  showMigrationTip = false,
  onDismissMigrationTip,
}: {
  children: ReactNode;
  indexStatus?: IndexStatus;
  showMigrationTip?: boolean;
  onDismissMigrationTip?: () => void;
}) {
  const { t } = useTranslation();
  const page = useUiStore((state) => state.page);
  const dataView = useUiStore((state) => state.dataView);
  const setPage = useUiStore((state) => state.setPage);
  const mainRef = useRef<HTMLElement>(null);
  useEffect(() => {
    mainRef.current?.scrollTo({ top: 0, left: 0 });
  }, [page, dataView]);
  const navigate = (id: PageKey) => {
    if (id === "data" && page === "data" && dataView === "sessions") {
      useUiStore.getState().closeSessions();
      return;
    }
    setPage(id);
    if (id !== "data") {
      useUiStore.getState().closeSessions();
    }
  };

  const links = (items: typeof primary) => items.map(({ id, icon: Icon }) => (
    <button key={id} className={page === id ? "active" : ""} onClick={() => navigate(id)} aria-current={page === id ? "page" : undefined}>
      <Icon size={17} strokeWidth={1.85} />
      <span>{t(`navigation.${id}`)}</span>
      {id === "data" && indexStatus?.running ? <i className="nav-live" aria-hidden="true" /> : null}
    </button>
  ));

  return (
    <div className="app-shell">
      <div className="titlebar-drag" data-tauri-drag-region />
      <aside className="sidebar">
        <button className="brand" onClick={() => navigate("vcti")} aria-label={t("app.name")}>
          <span className="brand-mark"><img src={appIconUrl} alt="" aria-hidden="true" /></span>
          <span><strong>{t("app.name")}</strong><small>{t("app.tagline")}</small></span>
        </button>
        <nav className="sidebar-nav primary-nav" aria-label={t("navigation.primary")}>{links(primary)}</nav>
        <div className="sidebar-spacer" />
        {indexStatus?.running ? (
          <div className="index-chip">
            <span className="pulse-dot" />
            <span>{t("sources.indexProgress", { processed: indexStatus.processedFiles, total: indexStatus.discoveredFiles })}</span>
          </div>
        ) : null}
        <nav className="sidebar-nav utility-nav" aria-label={t("navigation.utility")}>{links(utility)}</nav>
        <div className="privacy-note">{t("app.localOnly")}</div>
      </aside>
      <main ref={mainRef} className="main-content">
        {showMigrationTip && onDismissMigrationTip ? <IaMigrationTip onDismiss={onDismissMigrationTip} /> : null}
        {children}
      </main>
    </div>
  );
}
