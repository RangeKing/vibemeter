import { ArrowRight, ExternalLink, ShieldCheck, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { formatDateTime } from "../../lib/format";
import type {
  Locale,
  MemoryAccess,
  MemoryLedgerEvidenceReference,
  MemoryLedgerResponse,
} from "../../types";

export function MemoryEvidenceDrawer({
  ledger,
  access,
  locale,
  onClose,
  onOpenSession,
  onOpenProcessEvidence,
}: {
  ledger: MemoryLedgerResponse;
  access: MemoryAccess;
  locale: Locale;
  onClose: () => void;
  onOpenSession: (sessionId: string) => void;
  onOpenProcessEvidence: (evidence: MemoryLedgerEvidenceReference) => void;
}) {
  const { t } = useTranslation();
  const evidenceIds = new Set(access.evidenceIds);
  const evidence = ledger.evidence.filter((item) => evidenceIds.has(item.canonicalEventId));
  return (
    <aside className="memory-evidence-drawer" aria-labelledby="memory-evidence-title">
      <header>
        <div>
          <span className="eyebrow">{t("memory.evidence.eyebrow")}</span>
          <h4 id="memory-evidence-title">{t(`memory.operation.${access.operation}`)}</h4>
        </div>
        <button className="icon-button" onClick={onClose} aria-label={t("actions.close")}><X size={15} /></button>
      </header>
      <div className="memory-evidence-summary">
        <span className={`evidence-${access.evidenceLevel}`}><ShieldCheck size={12} />{t(`delegation.evidence.${access.evidenceLevel}`)}</span>
        <span>{t("memory.evidence.confidence", { value: Math.round(access.confidence * 100) })}</span>
        <code>{access.sourceCoverage}</code>
      </div>
      <dl className="memory-access-detail">
        <div><dt>{t("memory.detail.operation")}</dt><dd>{t(`memory.operation.${access.operation}`)}</dd></div>
        <div><dt>{t("memory.detail.time")}</dt><dd>{access.occurredAt ? formatDateTime(access.occurredAt, locale) : t("metrics.unavailable")}</dd></div>
        <div><dt>{t("memory.detail.algorithm")}</dt><dd>{access.algorithmVersion}</dd></div>
        {access.sessionId ? <div><dt>{t("memory.detail.session")}</dt><dd><button onClick={() => onOpenSession(access.sessionId!)}>{t("memory.openSession")}<ExternalLink size={11} /></button></dd></div> : null}
      </dl>
      <section className="memory-evidence-list">
        <h5>{t("memory.evidence.references", { count: evidence.length })}</h5>
        {evidence.map((item) => (
          <article key={item.id}>
            <span className={`evidence-${item.evidenceLevel}`}>{t(`delegation.evidence.${item.evidenceLevel}`)}</span>
            <div>
              <strong>{t("memory.evidence.access")}</strong>
              <small>{t(`memory.eventType.${item.eventType}`)} · {item.occurredAt ? formatDateTime(item.occurredAt, locale) : t("metrics.unavailable")}</small>
              <code>{item.canonicalEventId.slice(0, 12)}</code>
            </div>
            <span className="memory-evidence-actions">
              <button onClick={() => onOpenProcessEvidence(item)}>{t("memory.openProcess")}<ArrowRight size={11} /></button>
              {item.sessionId ? <button onClick={() => onOpenSession(item.sessionId!)}>{t("memory.openSession")}<ExternalLink size={11} /></button> : null}
            </span>
          </article>
        ))}
      </section>
    </aside>
  );
}
