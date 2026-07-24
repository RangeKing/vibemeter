import { isTauri } from "@tauri-apps/api/core";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import React from "react";
import ReactDOM from "react-dom/client";
import { I18nextProvider, useTranslation } from "react-i18next";
import { App } from "./App";
import i18n from "./i18n";
import "./styles.css";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 15_000,
      retry: 1,
      refetchOnWindowFocus: true,
      networkMode: "always",
    },
  },
});

function OfflineShell() {
  const { t } = useTranslation();
  return <main className="offline-shell"><span>T</span><h1>{t("app.name")}</h1><p>{t("errors.offlineShell")}</p></main>;
}

const surface = new URLSearchParams(window.location.search).get("surface") === "menubar" ? "menubar" : "main";
document.documentElement.dataset.surface = surface;

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <I18nextProvider i18n={i18n}>
      <QueryClientProvider client={queryClient}>
        {isTauri() ? <App surface={surface} /> : <OfflineShell />}
      </QueryClientProvider>
    </I18nextProvider>
  </React.StrictMode>,
);
