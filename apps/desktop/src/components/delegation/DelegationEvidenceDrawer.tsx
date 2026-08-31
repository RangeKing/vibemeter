import { ArrowRight, ExternalLink, ShieldCheck, X } from "lucide-react";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import type {
  DelegationEvidenceReference,
  DelegationTraceResponse,
  Locale,
} from "../../types";
import { formatDateTime } from "../../lib/format";
import type { DelegationSelection } from "./delegationGraphModel";

export function DelegationEvidenceDrawer({
  trace,
  selection,
  locale,
  onClose,
  onOpenSession,
  onOpenProcessEvidence,
}: {
  trace: DelegationTraceResponse;
  selection: DelegationSelection;
  locale: Locale;
  onClose: () => void;
  onOpenSession: (sessionId: string) => void;
  onOpenProcessEvidence: (evidence: DelegationEvidenceReference) => void;
}) {
  const { t } = useTranslation();
  const selectedNode = selection.type === "node"
    ? trace.nodes.find((node) => node.id === selection.id)
    : undefined;
  const selectedEdge = selection.type === "edge"
    ? trace.edges.find((edge) => edge.id === selection.id)
    : undefined;
  const evidenceIds = useMemo(() => {
    if (selectedEdge) return new Set(selectedEdge.evidenceIds);
    if (!selectedNode) return new Set<string>();
    return new Set(
      trace.edges
        .filter((edge) => edge.from === selectedNode.id || edge.to === selectedNode.id)
        .flatMap((edge) => edge.evidenceIds),
    );
  }, [selectedEdge, selectedNode, trace.edges]);
  const evidence = trace.evidence.filter((item) => evidenceIds.has(item.canonicalEventId));
  const evidenceLevel = selectedEdge?.evidenceLevel ?? selectedNode?.evidenceLevel ?? "unavailable";
  const sourceCoverage = selectedEdge?.sourceCoverage ?? selectedNode?.sourceCoverage ?? "not-recorded";
  const confidence = selectedEdge?.confidence ?? selectedNode?.confidence ?? 0;
  const title = selectedNode?.safeLabel
    ?? (selectedEdge ? t(`delegation.relation.${selectedEdge.relationType}`) : t("delegation.evidence.title"));

  return (
    <aside className="delegation-evidence-drawer" aria-labelledby="delegation-evidence-title">
      <header>
        <div>
          <span className="eyebrow">{t("delegation.evidence.eyebrow")}</span>
          <h4 id="delegation-evidence-title">{title}</h4>
        </div>
        <button className="icon-button" onClick={onClose} aria-label={t("actions.close")}><X size={15} /></button>
      </header>
      <div className="delegation-evidence-summary">
        <span className={`evidence-${evidenceLevel}`}><ShieldCheck size={12} />{t(`delegation.evidence.${evidenceLevel}`)}</span>
        <span>{t("delegation.evidence.confidence", { value: Math.round(confidence * 100) })}</span>
        <span>{sourceCoverage}</span>
      </div>
      {selectedNode ? (
        <dl className="delegation-node-detail">
          <div><dt>{t("delegation.detail.kind")}</dt><dd>{t(`delegation.nodeKind.${selectedNode.kind}`)}</dd></div>
          <div><dt>{t("delegation.detail.status")}</dt><dd>{t(`delegation.status.${selectedNode.status}`)}</dd></div>
          <div><dt>{t("delegation.detail.outcome")}</dt><dd>{selectedNode.outcome ? t(`delegation.outcome.${selectedNode.outcome}`, { defaultValue: selectedNode.outcome }) : t("metrics.unavailable")}</dd></div>
          {selectedNode.sessionId ? <div><dt>{t("delegation.detail.session")}</dt><dd><button onClick={() => onOpenSession(selectedNode.sessionId!)}>{t("delegation.openSession")}<ExternalLink size={11} /></button></dd></div> : null}
        </dl>
      ) : selectedEdge ? (
        <dl className="delegation-node-detail">
          <div><dt>{t("delegation.detail.relation")}</dt><dd>{t(`delegation.relation.${selectedEdge.relationType}`)}</dd></div>
          <div><dt>{t("delegation.detail.status")}</dt><dd>{t(`delegation.status.${selectedEdge.status}`)}</dd></div>
          <div><dt>{t("delegation.detail.algorithm")}</dt><dd>{selectedEdge.algorithmVersion}</dd></div>
        </dl>
      ) : null}
      <section className="delegation-evidence-list">
        <h5>{t("delegation.evidence.references", { count: evidence.length })}</h5>
        {evidence.map((item) => (
          <article key={item.id}>
            <span className={`evidence-${item.evidenceLevel}`}>{t(`delegation.evidence.${item.evidenceLevel}`)}</span>
            <div>
              <strong>{t(`delegation.evidenceRole.${item.role}`, { defaultValue: item.role })}</strong>
              <small>{t(`delegation.eventType.${item.eventType}`)} · {item.occurredAt ? formatDateTime(item.occurredAt, locale) : t("metrics.unavailable")}</small>
              <code>{item.canonicalEventId.slice(0, 12)}</code>
            </div>
            <span className="delegation-evidence-actions">
              <button onClick={() => onOpenProcessEvidence(item)}>{t("delegation.openProcess")}<ArrowRight size={11} /></button>
              {item.sessionId ? <button onClick={() => onOpenSession(item.sessionId!)}>{t("delegation.openSession")}<ExternalLink size={11} /></button> : null}
            </span>
          </article>
        ))}
        {!evidence.length ? <p className="quiet-empty">{t("delegation.evidence.none")}</p> : null}
      </section>
    </aside>
  );
}
