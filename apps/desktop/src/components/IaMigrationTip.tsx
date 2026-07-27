import { ArrowRight, Database, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useUiStore } from "../store";
import type { PageKey } from "../types";

export function IaMigrationTip({ onDismiss }: { onDismiss: () => void }) {
  const { t } = useTranslation();
  const setPage = useUiStore((state) => state.setPage);
  const openSessions = useUiStore((state) => state.openSessions);
  const open = (page: PageKey) => {
    setPage(page);
  };

  return (
    <aside className="ia-migration-tip" role="status">
      <div>
        <strong>{t("migration.title")}</strong>
        <p>{t("migration.body")}</p>
        <div className="ia-migration-links">
          <button className="button subtle" onClick={() => openSessions()}>{t("navigation.sessions")}<ArrowRight size={13} /></button>
          <button className="button subtle" onClick={() => open("sources")}><Database size={14} />{t("navigation.sources")}<ArrowRight size={13} /></button>
          <button className="button subtle" onClick={() => open("data")}><ArrowRight size={14} />{t("navigation.data")}</button>
        </div>
      </div>
      <button className="button primary" onClick={onDismiss}>{t("migration.dismiss")}</button>
      <button className="icon-button ia-migration-close" onClick={onDismiss} aria-label={t("actions.close")}><X size={16} /></button>
    </aside>
  );
}
