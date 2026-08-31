import { BrainCircuit, DatabaseZap, Info, RefreshCw, ShieldAlert } from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../../lib/api";
import type { Locale, MemoryLedgerEvidenceReference } from "../../types";
import { ErrorState, LoadingState } from "../ui";
import { MemoryEvidenceDrawer } from "./MemoryEvidenceDrawer";
import { MemoryFilters, type MemoryFilterValue } from "./MemoryFilters";
import { MemoryTimeline } from "./MemoryTimeline";

export function MemoryLedgerView({
  sessionId,
  locale,
  onOpenSession,
  onOpenProcessEvidence,
}: {
  sessionId: string;
  locale: Locale;
  onOpenSession: (sessionId: string) => void;
  onOpenProcessEvidence: (evidence: MemoryLedgerEvidenceReference) => void;
}) {
  const { t } = useTranslation();
  const [filter, setFilter] = useState<MemoryFilterValue>("all");
  const [selectedId, setSelectedId] = useState<string>();
  const query = useQuery({
    queryKey: ["memory-ledger", sessionId],
    queryFn: () => api.memoryLedger(sessionId),
  });

  useEffect(() => {
    setFilter("all");
    setSelectedId(undefined);
  }, [sessionId]);

  const filtered = useMemo(
    () => query.data?.accesses.filter((access) => filter === "all" || access.operation === filter) ?? [],
    [filter, query.data],
  );
  const selected = query.data?.accesses.find((access) => access.id === selectedId);

  useEffect(() => {
    if (selectedId && !filtered.some((access) => access.id === selectedId)) setSelectedId(undefined);
  }, [filtered, selectedId]);

  if (query.isLoading) {
    return <section className="memory-state"><LoadingState /><p>{t("memory.loading")}</p></section>;
  }
  if (query.isError || !query.data) {
    return <section className="memory-state"><ErrorState retry={() => void query.refetch()} /><p>{t("memory.error")}</p></section>;
  }

  const ledger = query.data;
  if (ledger.status === "not-recorded") {
    return (
      <section className="memory-not-recorded" role="status">
        <span><DatabaseZap size={22} /></span>
        <div>
          <h3>{t("memory.notRecordedTitle")}</h3>
          <p>{t("memory.notRecordedBody")}</p>
          <small>{t("memory.capability", { value: t(`sources.signalCapabilities.${ledger.coverage.capability}`) })}</small>
          {ledger.coverage.unavailableOperations.length ? (
            <ul>{ledger.coverage.unavailableOperations.map((operation) => <li key={operation}>{t(`memory.operation.${operation}`)}</li>)}</ul>
          ) : null}
        </div>
        <button className="button secondary" onClick={() => void query.refetch()}><RefreshCw size={13} />{t("actions.retry")}</button>
      </section>
    );
  }

  return (
    <section className="memory-ledger-view">
      <header className="memory-ledger-header">
        <div>
          <span className="eyebrow"><BrainCircuit size={12} />{t("memory.eyebrow")}</span>
          <h3>{t("memory.title")}</h3>
          <p>{t("memory.body")}</p>
        </div>
        <div className={`memory-ledger-status status-${ledger.status}`}>
          {ledger.status === "ready" ? <Info size={13} /> : <ShieldAlert size={13} />}
          <span>{t(`memory.ledgerStatus.${ledger.status}`)}</span>
          <small>{ledger.algorithmVersion}</small>
        </div>
      </header>

      {ledger.status === "partial" ? (
        <div className="memory-partial-notice" role="status">
          <ShieldAlert size={15} />
          <p><strong>{t("memory.partialTitle")}</strong>{t("memory.partialBody")}</p>
        </div>
      ) : null}

      <div className="memory-coverage" aria-label={t("memory.coverage.title")}>
        <span><strong>{ledger.coverage.accessCount}</strong>{t("memory.coverage.accesses")}</span>
        <span><strong>{ledger.coverage.readCount}</strong>{t("memory.operation.read")}</span>
        <span><strong>{ledger.coverage.writeCount}</strong>{t("memory.operation.write")}</span>
        <span className="derived"><strong>{ledger.coverage.derivedCount}</strong>{t("delegation.evidence.derived")}</span>
        <span><strong>{ledger.coverage.evidenceCount}</strong>{t("memory.coverage.evidence")}</span>
        <code>{ledger.coverage.sourceCoverage}</code>
      </div>

      <MemoryFilters accesses={ledger.accesses} value={filter} onChange={setFilter} />

      <div className={`memory-workspace ${selected ? "has-drawer" : ""}`}>
        <MemoryTimeline
          accesses={filtered}
          locale={locale}
          selectedId={selectedId}
          onSelect={setSelectedId}
        />
        {selected ? (
          <MemoryEvidenceDrawer
            ledger={ledger}
            access={selected}
            locale={locale}
            onClose={() => setSelectedId(undefined)}
            onOpenSession={onOpenSession}
            onOpenProcessEvidence={onOpenProcessEvidence}
          />
        ) : null}
      </div>
    </section>
  );
}
