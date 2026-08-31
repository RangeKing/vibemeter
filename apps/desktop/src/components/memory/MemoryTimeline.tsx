import { BookOpen, Clock3, SquarePen } from "lucide-react";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { formatDateTime } from "../../lib/format";
import type { Locale, MemoryAccess } from "../../types";

function timestamp(value: string | undefined): number | undefined {
  if (!value) return undefined;
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : undefined;
}

export function MemoryTimeline({
  accesses,
  locale,
  selectedId,
  onSelect,
}: {
  accesses: MemoryAccess[];
  locale: Locale;
  selectedId?: string;
  onSelect: (id: string) => void;
}) {
  const { t } = useTranslation();
  const scale = useMemo(() => {
    const values = accesses
      .map((access) => timestamp(access.occurredAt))
      .filter((value): value is number => value !== undefined);
    const minimum = values.length ? Math.min(...values) : 0;
    const maximum = values.length ? Math.max(...values) : minimum;
    return { minimum, duration: Math.max(1, maximum - minimum) };
  }, [accesses]);

  if (!accesses.length) {
    return <div className="memory-timeline-empty" role="status">{t("memory.empty")}</div>;
  }

  return (
    <section className="memory-timeline" aria-labelledby="memory-timeline-title">
      <header>
        <div>
          <h4 id="memory-timeline-title">{t("memory.timeline.title")}</h4>
          <p>{t("memory.timeline.body")}</p>
        </div>
        <span><Clock3 size={13} />{t("memory.timeline.count", { count: accesses.length })}</span>
      </header>
      <div className="memory-timeline-axis" aria-hidden="true"><i /><i /><i /><i /><i /></div>
      <div className="memory-access-list">
        {accesses.map((access, index) => {
          const occurred = timestamp(access.occurredAt);
          const left = occurred === undefined ? 0 : ((occurred - scale.minimum) / scale.duration) * 100;
          const selected = selectedId === access.id;
          const Icon = access.operation === "write" ? SquarePen : BookOpen;
          return (
            <button
              key={access.id}
              className={`memory-access-row operation-${access.operation} evidence-${access.evidenceLevel} ${selected ? "selected" : ""}`}
              onClick={() => onSelect(access.id)}
              aria-label={`${t(`memory.operation.${access.operation}`)} · ${access.occurredAt ? formatDateTime(access.occurredAt, locale) : t("metrics.unavailable")} · ${t(`delegation.evidence.${access.evidenceLevel}`)}`}
            >
              <span className="memory-access-sequence">{String(index + 1).padStart(2, "0")}</span>
              <span className="memory-access-icon"><Icon size={14} /></span>
              <span className="memory-access-copy">
                <strong>{t(`memory.operation.${access.operation}`)}</strong>
                <small>{access.occurredAt ? formatDateTime(access.occurredAt, locale) : t("metrics.unavailable")}</small>
              </span>
              <span className="memory-access-track" aria-hidden="true"><i style={{ left: `${left}%` }} /></span>
              <span className={`memory-access-evidence evidence-${access.evidenceLevel}`}>{t(`delegation.evidence.${access.evidenceLevel}`)}</span>
            </button>
          );
        })}
      </div>
    </section>
  );
}
