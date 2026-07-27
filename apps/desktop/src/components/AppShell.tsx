import {
  BarChart3,
  Database,
  History,
  Lightbulb,
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

const primary: Array<{ id: PageKey; icon: typeof Sparkles }> = [
  { id: "data", icon: BarChart3 },
  { id: "live", icon: RadioTower },
  { id: "vcti", icon: ScanFace },
];

const history: Array<{ id: PageKey; icon: typeof Sparkles }> = [
  { id: "insights", icon: Lightbulb },
  { id: "sessions", icon: History },
];

const utility: Array<{ id: PageKey; icon: typeof Sparkles }> = [
  { id: "share", icon: Share2 },
  { id: "sources", icon: Database },
  { id: "settings", icon: Settings },
];

export function AppShell({ children, indexStatus }: { children: ReactNode; indexStatus?: IndexStatus }) {
  const { t } = useTranslation();
  const page = useUiStore((state) => state.page);
  const setPage = useUiStore((state) => state.setPage);
  const mainRef = useRef<HTMLElement>(null);
  useEffect(() => {
    mainRef.current?.scrollTo({ top: 0, left: 0 });
  }, [page]);
  const navigate = (id: PageKey) => {
    setPage(id);
    if (id !== "sessions") useUiStore.getState().selectSession(undefined);
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
        <button className="brand" onClick={() => navigate("data")} aria-label={t("app.name")}>
          <span className="brand-mark"><img src={appIconUrl} alt="" aria-hidden="true" /></span>
          <span><strong>{t("app.name")}</strong><small>{t("app.tagline")}</small></span>
        </button>
        <nav className="sidebar-nav primary-nav" aria-label="Primary">{links(primary)}</nav>
        <nav className="sidebar-nav history-nav" aria-label="History">{links(history)}</nav>
        <div className="sidebar-spacer" />
        {indexStatus?.running ? (
          <div className="index-chip">
            <span className="pulse-dot" />
            <span>{t("sources.indexProgress", { processed: indexStatus.processedFiles, total: indexStatus.discoveredFiles })}</span>
          </div>
        ) : null}
        <nav className="sidebar-nav utility-nav" aria-label="Utility">{links(utility)}</nav>
        <div className="privacy-note">{t("app.localOnly")}</div>
      </aside>
      <main ref={mainRef} className="main-content">{children}</main>
    </div>
  );
}
