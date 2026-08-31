import { BookOpen, RotateCcw, SquarePen } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { MemoryAccess, MemoryOperation } from "../../types";

export type MemoryFilterValue = "all" | MemoryOperation;

export function MemoryFilters({
  accesses,
  value,
  onChange,
}: {
  accesses: MemoryAccess[];
  value: MemoryFilterValue;
  onChange: (value: MemoryFilterValue) => void;
}) {
  const { t } = useTranslation();
  const readCount = accesses.filter((access) => access.operation === "read").length;
  const writeCount = accesses.filter((access) => access.operation === "write").length;
  return (
    <div className="memory-filters" aria-label={t("memory.filters.title")}>
      <button className={value === "all" ? "active" : ""} onClick={() => onChange("all")}>
        <RotateCcw size={13} />
        <span>{t("memory.filters.all")}</span>
        <strong>{accesses.length}</strong>
      </button>
      <button className={value === "read" ? "active" : ""} disabled={!readCount} onClick={() => onChange("read")}>
        <BookOpen size={13} />
        <span>{t("memory.operation.read")}</span>
        <strong>{readCount}</strong>
      </button>
      <button className={value === "write" ? "active" : ""} disabled={!writeCount} onClick={() => onChange("write")}>
        <SquarePen size={13} />
        <span>{t("memory.operation.write")}</span>
        <strong>{writeCount}</strong>
      </button>
    </div>
  );
}
