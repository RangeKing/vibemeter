import { AlertTriangle, DatabaseZap, GitFork, Info, RefreshCw, ShieldAlert } from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { DelegationEvidenceReference, Locale } from "../../types";
import { api } from "../../lib/api";
import { ErrorState, LoadingState } from "../ui";
import { DelegationEvidenceDrawer } from "./DelegationEvidenceDrawer";
import { DelegationFilters } from "./DelegationFilters";
import { DelegationGraph } from "./DelegationGraph";
import { DelegationTimeline } from "./DelegationTimeline";
import {
  filterDelegationTrace,
  type DelegationFilterValue,
  type DelegationSelection,
} from "./delegationGraphModel";

const emptyFilters: DelegationFilterValue = { agent: "all", relationType: "all", status: "all" };

export function DelegationTraceView({
  sessionId,
  locale,
  onOpenSession,
  onOpenProcessEvidence,
}: {
  sessionId: string;
  locale: Locale;
  onOpenSession: (sessionId: string) => void;
  onOpenProcessEvidence: (evidence: DelegationEvidenceReference) => void;
}) {
  const { t } = useTranslation();
  const [filters, setFilters] = useState<DelegationFilterValue>(emptyFilters);
  const [selection, setSelection] = useState<DelegationSelection>();
  const query = useQuery({
    queryKey: ["delegation-trace", sessionId],
    queryFn: () => api.delegationTrace(sessionId),
  });

  useEffect(() => {
    setFilters(emptyFilters);
    setSelection(undefined);
  }, [sessionId]);

  const filtered = useMemo(
    () => query.data ? filterDelegationTrace(query.data, filters) : { nodes: [], edges: [] },
    [filters, query.data],
  );

  if (query.isLoading) {
    return <section className="delegation-state"><LoadingState /><p>{t("delegation.loading")}</p></section>;
  }
  if (query.isError || !query.data) {
    return <section className="delegation-state"><ErrorState retry={() => void query.refetch()} /><p>{t("delegation.error")}</p></section>;
  }

  const trace = query.data;
  if (trace.status === "not-recorded") {
    return (
      <section className="delegation-not-recorded" role="status">
        <span><DatabaseZap size={22} /></span>
        <div>
          <h3>{t("delegation.notRecordedTitle")}</h3>
          <p>{t("delegation.notRecordedBody")}</p>
          <small>{t("delegation.capability", { value: t(`sources.signalCapabilities.${trace.coverage.capability}`) })}</small>
          {trace.coverage.unavailableSignals.length ? (
            <ul>{trace.coverage.unavailableSignals.map((signal) => <li key={signal}>{t(`delegation.signal.${signal}`)}</li>)}</ul>
          ) : null}
        </div>
        <button className="button secondary" onClick={() => void query.refetch()}><RefreshCw size={13} />{t("actions.retry")}</button>
      </section>
    );
  }

  const selectedVisible = selection?.type === "node"
    ? filtered.nodes.some((node) => node.id === selection.id)
    : selection?.type === "edge"
      ? filtered.edges.some((edge) => edge.id === selection.id)
      : true;
  const activeSelection = selectedVisible ? selection : undefined;

  return (
    <section className="delegation-trace-view">
      <header className="delegation-trace-header">
        <div>
          <span className="eyebrow"><GitFork size={12} />{t("delegation.eyebrow")}</span>
          <h3>{t("delegation.title")}</h3>
          <p>{t("delegation.body")}</p>
        </div>
        <div className={`delegation-trace-status status-${trace.status}`}>
          {trace.status === "ready" ? <Info size={13} /> : <ShieldAlert size={13} />}
          <span>{t(`delegation.traceStatus.${trace.status}`)}</span>
          <small>{trace.algorithmVersion}</small>
        </div>
      </header>

      {trace.status === "partial" ? (
        <div className="delegation-partial-notice" role="status">
          <ShieldAlert size={15} />
          <p><strong>{t("delegation.partialTitle")}</strong>{t("delegation.partialBody")}</p>
        </div>
      ) : null}

      <div className="delegation-coverage" aria-label={t("delegation.coverage.title")}>
        <span><strong>{trace.coverage.relationCount}</strong>{t("delegation.coverage.relations")}</span>
        <span><strong>{trace.coverage.evidenceCount}</strong>{t("delegation.coverage.evidence")}</span>
        <span className="observed"><strong>{trace.coverage.observedCount}</strong>{t("delegation.evidence.observed")}</span>
        <span className="derived"><strong>{trace.coverage.derivedCount}</strong>{t("delegation.evidence.derived")}</span>
        <span className="inferred"><strong>{trace.coverage.inferredCount}</strong>{t("delegation.evidence.inferred")}</span>
        <code>{trace.coverage.sourceCoverage}</code>
      </div>

      <DelegationFilters nodes={trace.nodes} edges={trace.edges} value={filters} onChange={setFilters} />

      <div className={`delegation-workspace ${activeSelection ? "has-drawer" : ""}`}>
        <div className="delegation-main">
          <DelegationGraph
            nodes={filtered.nodes}
            edges={filtered.edges}
            selection={activeSelection}
            onSelection={setSelection}
          />
          <DelegationTimeline
            nodes={filtered.nodes}
            edges={filtered.edges}
            locale={locale}
            selection={activeSelection}
            onSelection={setSelection}
          />
          {trace.anomalies.length ? (
            <section className="delegation-anomalies" aria-labelledby="delegation-anomalies-title">
              <header><AlertTriangle size={15} /><h4 id="delegation-anomalies-title">{t("delegation.anomalies.title")}</h4><span>{trace.anomalies.length}</span></header>
              <div>
                {trace.anomalies.map((anomaly) => (
                  <button
                    className={`severity-${anomaly.severity}`}
                    key={anomaly.id}
                    onClick={() => {
                      const edgeId = anomaly.edgeIds[0];
                      const nodeId = anomaly.nodeIds[0];
                      if (edgeId) setSelection({ type: "edge", id: edgeId });
                      else if (nodeId) setSelection({ type: "node", id: nodeId });
                    }}
                  >
                    <AlertTriangle size={13} />
                    <span><strong>{t(`delegation.anomaly.${anomaly.kind}`)}</strong><small>{t("delegation.anomalies.evidence", { count: anomaly.evidenceIds.length })} · {anomaly.ruleVersion}</small></span>
                    <em>{Math.round(anomaly.confidence * 100)}%</em>
                  </button>
                ))}
              </div>
            </section>
          ) : null}
        </div>
        {activeSelection ? (
          <DelegationEvidenceDrawer
            trace={trace}
            selection={activeSelection}
            locale={locale}
            onClose={() => setSelection(undefined)}
            onOpenSession={onOpenSession}
            onOpenProcessEvidence={onOpenProcessEvidence}
          />
        ) : null}
      </div>
    </section>
  );
}
