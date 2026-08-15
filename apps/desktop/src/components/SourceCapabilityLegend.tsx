import { useTranslation } from "react-i18next";
import { sourceCapabilityNameGroups } from "../lib/sourceStatus";
import type { Locale } from "../types";

export function SourceCapabilityLegend({ locale }: { locale: Locale }) {
  const { t } = useTranslation();
  const names = sourceCapabilityNameGroups(locale === "zh-CN" ? "、" : ", ");
  const groups = [
    ["exact", names.exact],
    ["experimental", names.experimental],
    ["historyOnly", names.historyOnly],
  ] as const;

  return (
    <aside className="data-source-capabilities" aria-label={t("data.sourceCapabilities.title")}>
      <strong>{t("data.sourceCapabilities.title")}</strong>
      {groups.filter(([, sourceNames]) => sourceNames.length > 0).map(([capability, sourceNames]) => (
        <span key={capability} className={`capability-${capability}`}>
          <b>{t(`data.sourceCapabilities.${capability}`)}</b>
          <em>{sourceNames}</em>
        </span>
      ))}
    </aside>
  );
}
