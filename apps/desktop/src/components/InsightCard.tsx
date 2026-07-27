import { ArrowUpRight, Clock3, FileCode2, Lightbulb } from "lucide-react";
import { useTranslation } from "react-i18next";
import { formatCompact, formatPercent } from "../lib/format";
import type { InsightItem, Locale } from "../types";

function splitEvidenceLabel(label: string): [string, string] {
  const divider = label.lastIndexOf(" · ");
  return divider < 0 ? [label, ""] : [label.slice(0, divider), label.slice(divider + 3)];
}

function peakHourLabel(label: string): { window: string; count: number } | undefined {
  const [time, countText] = splitEvidenceLabel(label);
  const hour = Number.parseInt(time.split(":")[0] ?? "", 10);
  const count = Number.parseInt(countText, 10);
  if (!Number.isFinite(hour) || !Number.isFinite(count)) return undefined;
  const nextHour = (hour + 1) % 24;
  return { window: `${String(hour).padStart(2, "0")}:00–${String(nextHour).padStart(2, "0")}:00`, count };
}

export function InsightCard({
  item,
  index,
  locale,
  onOpenSession,
}: {
  item: InsightItem;
  index: number;
  locale: Locale;
  onOpenSession?: (sessionId: string) => void;
}) {
  const { t } = useTranslation();
  const fileEvidence = item.id === "high-churn-file" ? item.evidence.find((evidence) => evidence.kind === "file") : undefined;
  const peakEvidence = item.id === "peak-hour" ? item.evidence.find((evidence) => evidence.kind === "sessions") : undefined;
  const [filePath, editCount] = fileEvidence ? splitEvidenceLabel(fileEvidence.label) : ["", ""];
  const peak = peakEvidence ? peakHourLabel(peakEvidence.label) : undefined;
  const remainingEvidence = item.evidence.filter((evidence) => evidence !== fileEvidence && evidence !== peakEvidence);

  return (
    <article className={`insight-card tier-${item.tier}`}>
      <header>
        <span className="insight-number">{String(index + 1).padStart(2, "0")}</span>
        <span className="finding-tier">{t(`insights.${item.tier}`)}</span>
      </header>
      <Lightbulb size={21} />
      <h2>{t(item.titleKey)}</h2>
      <p>{t(item.detailKey)}</p>
      {fileEvidence ? (
        <button
          className="insight-file-link"
          disabled={!item.targetSessionId}
          onClick={() => {
            if (!item.targetSessionId || !onOpenSession) return;
            onOpenSession(item.targetSessionId);
          }}
        >
          <FileCode2 size={23} />
          <span>
            <strong>{filePath}</strong>
            <small>{t("insights.fileActivity", { edits: Number.parseInt(editCount, 10) || 0, sessions: item.value ?? 0 })}</small>
          </span>
          <span className="insight-file-action">{t("insights.openSession")}<ArrowUpRight size={15} /></span>
        </button>
      ) : null}
      {peak ? (
        <div className="insight-peak-window">
          <Clock3 size={24} />
          <span><strong>{peak.window}</strong><small>{t("insights.peakHourSessions", { count: peak.count })}</small></span>
        </div>
      ) : null}
      {item.value != null && !fileEvidence ? (
        <strong className="insight-value">
          {item.value < 1 && item.id.includes("gap") ? formatPercent(item.value, locale) : formatCompact(item.value, locale)}
        </strong>
      ) : null}
      {remainingEvidence.length ? <div className="evidence-chips">{remainingEvidence.map((evidence) => <span key={evidence.id}>{evidence.kind} · {evidence.label}</span>)}</div> : null}
    </article>
  );
}
