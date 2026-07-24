import { useTranslation } from "react-i18next";
import { useUiStore } from "../store";
import type { RangeKey } from "../types";

const ranges: RangeKey[] = ["today", "7d", "30d", "180d", "year"];

export function RangePicker({ compact = false }: { compact?: boolean }) {
  const { t } = useTranslation();
  const range = useUiStore((state) => state.range);
  const setRange = useUiStore((state) => state.setRange);
  return (
    <div className={compact ? "segmented compact range-picker" : "segmented range-picker"} aria-label={t("data.rangeLabel")}>
      {ranges.map((item) => (
        <button
          key={item}
          className={range === item ? "active" : ""}
          aria-pressed={range === item}
          onClick={() => setRange(item)}
        >
          {t(`ranges.${item}`)}
        </button>
      ))}
    </div>
  );
}
