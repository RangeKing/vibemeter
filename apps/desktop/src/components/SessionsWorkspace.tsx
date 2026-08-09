import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowLeft, CheckCircle2, ChevronRight, FileCode2, GitBranch, GitCommitHorizontal, Search, Split, X } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { RangePicker } from "./RangePicker";
import { AgentBadge, EmptyState, ErrorState, LoadingState, PageHeader, SessionEvidence, SessionTitle, VerificationPill } from "./ui";
import { api } from "../lib/api";
import { formatCompact, formatDateTime, formatDuration, tokenTotal } from "../lib/format";
import { useUiStore } from "../store";
import type { Locale, SessionDetail, SessionSummary } from "../types";

const PAGE_SIZE = 50;
const phaseKeys = new Set(["understand", "inspect", "edit", "verify", "fix", "plan", "execute"]);

function formatPhaseTime(value: string | undefined, locale: Locale): string | undefined {
  if (!value) return undefined;
  return new Intl.DateTimeFormat(locale, { hour: "2-digit", minute: "2-digit" }).format(new Date(value));
}

function SessionReplay({ detail, locale, onClose }: { detail: SessionDetail; locale: Locale; onClose: () => void }) {
  const { t } = useTranslation();
  const client = useQueryClient();
  const [showAllPhases, setShowAllPhases] = useState(false);
  const visiblePhases = showAllPhases ? detail.phases : detail.phases.slice(0, 24);
  const split = useMutation({
    mutationFn: () => api.splitSession(detail.id),
    onSuccess: async () => {
      await client.invalidateQueries({ queryKey: ["session", detail.id] });
      await client.invalidateQueries({ queryKey: ["sessions"] });
    },
  });
  return (
    <aside className="replay-panel">
      <header className="replay-header">
        <button className="icon-button replay-back" onClick={onClose} aria-label={t("actions.back")}><ArrowLeft size={17} /></button>
        <div><span className="eyebrow">{t("sessions.detailTitle")}</span><h2><SessionTitle session={detail} /></h2></div>
        <AgentBadge agent={detail.agent} model={detail.model} compact />
      </header>
      <div className="replay-meta">
        <span>{formatDateTime(detail.startedAt, locale)}</span><span>{formatDuration(detail.activeSeconds, locale)}</span><VerificationPill value={detail.verificationState} />
      </div>
      {detail.task ? <div className="task-assignment"><span>{detail.task.title}</span><button onClick={() => split.mutate()} disabled={split.isPending}><Split size={13} />{t("sessions.splitTask")}</button></div> : null}

      <section className="replay-section process-section">
        <header><div><h3>{t("sessions.process")}</h3><p>{t("sessions.processBody")}</p></div><span>{detail.phases.reduce((sum, phase) => sum + phase.eventCount, 0)} events</span></header>
        <div className="phase-timeline">
          {visiblePhases.map((phase, index) => {
            const failures = phase.events.filter((event) => event.success === false).length;
            const successes = phase.events.filter((event) => event.success === true).length;
            const durationMs = phase.events.reduce((sum, event) => sum + (event.durationMs ?? 0), 0);
            const phaseTime = formatPhaseTime(phase.startedAt, locale);
            return <article key={phase.id} className={`phase phase-${phase.phaseKey}`}>
              <div className="phase-axis"><span>{String(index + 1).padStart(2, "0")}</span></div>
              <div className="phase-body">
                <header><strong>{t(`sessions.phase.${phaseKeys.has(phase.phaseKey) ? phase.phaseKey : "other"}`)}</strong><span>{t("sessions.eventCount", { count: phase.eventCount })}</span></header>
                <div className="phase-meta">{phaseTime ? <span>{phaseTime}</span> : null}{durationMs ? <span>{formatDuration(Math.max(1, Math.round(durationMs / 1_000)), locale)}</span> : null}{successes ? <span className="successful">{t("sessions.successful", { count: successes })}</span> : null}{failures ? <span className="failed">{t("sessions.failed", { count: failures })}</span> : null}</div>
                <div className="event-chips">{phase.events.slice(0, 5).map((event) => <span key={`${event.sequence}-${event.name}`} className={event.success === false ? "failed" : ""}>{event.name}</span>)}{phase.events.length > 5 ? <span className="event-more">+{phase.events.length - 5}</span> : null}</div>
              </div>
            </article>;
          })}
          {!detail.phases.length ? <div className="quiet-empty">{t("metrics.unavailable")}</div> : null}
        </div>
        {detail.phases.length > 24 ? <button className="phase-expand" onClick={() => setShowAllPhases((value) => !value)}>{showAllPhases ? t("sessions.collapseTimeline") : t("sessions.expandTimeline", { count: detail.phases.length - 24 })}</button> : null}
      </section>

      <div className="replay-columns">
        <section className="replay-section">
          <header><div><h3>{t("sessions.files")}</h3></div><span>{detail.fileChanges.length}</span></header>
          <div className="file-evidence-list">
            {detail.fileChanges.map((file) => <div key={file.id}><FileCode2 size={15} /><span title={file.path}>{file.path}</span><strong>+{file.linesAdded} / −{file.linesDeleted}</strong><small>{file.finalState}</small></div>)}
            {!detail.fileChanges.length ? <div className="quiet-empty">{t("metrics.unavailable")}</div> : null}
          </div>
        </section>
        <section className="replay-section git-section">
          <header><div><h3>{t("sessions.git")}</h3></div>{detail.gitEvidence.branch ? <span><GitBranch size={12} />{detail.gitEvidence.branch}</span> : null}</header>
          {!detail.gitEvidence.available ? <div className="permission-empty"><GitBranch size={20} /><p>{t(detail.gitEvidence.state === "not-authorized" ? "sessions.gitNotAuthorized" : "sessions.gitUnavailable")}</p></div> : (
            <div className="commit-list">{detail.gitEvidence.commits.map((commit) => <div key={commit.hash}><GitCommitHorizontal size={14} /><span><strong>{commit.subject}</strong><small>{commit.hash.slice(0, 8)} · {formatDateTime(commit.committedAt, locale)}</small></span></div>)}</div>
          )}
        </section>
      </div>

      <section className="capability-strip"><strong>{t("sessions.capabilities")}</strong>{detail.capabilities.map((item) => <span key={item}><CheckCircle2 size={12} />{item}</span>)}</section>
    </aside>
  );
}

export function SessionsWorkspace({
  locale,
  embedded = false,
  onBack,
}: {
  locale: Locale;
  embedded?: boolean;
  onBack?: () => void;
}) {
  const { t } = useTranslation();
  const range = useUiStore((state) => state.range);
  const selectedId = useUiStore((state) => state.selectedSessionId);
  const selectSession = useUiStore((state) => state.selectSession);
  const [searchInput, setSearchInput] = useState("");
  const [search, setSearch] = useState("");
  const [agent, setAgent] = useState("");
  const [model, setModel] = useState("");
  const [project, setProject] = useState("");
  const [state, setState] = useState("");
  const [attentionOnly, setAttentionOnly] = useState(false);
  const [codeOnly, setCodeOnly] = useState(false);
  const [commitOnly, setCommitOnly] = useState(false);
  const [page, setPage] = useState(0);
  const [items, setItems] = useState<SessionSummary[]>([]);

  useEffect(() => {
    const handle = window.setTimeout(() => setSearch(searchInput.trim()), 250);
    return () => window.clearTimeout(handle);
  }, [searchInput]);

  useEffect(() => {
    setPage(0);
    setItems([]);
  }, [range, agent, search, model, project, state, attentionOnly, codeOnly, commitOnly]);

  const query = useQuery({
    queryKey: ["sessions", range, agent, search, model, project, state, attentionOnly, codeOnly, commitOnly, page],
    queryFn: () => api.sessions({
      range,
      agent: agent || undefined,
      search: search || undefined,
      model: model || undefined,
      project: project || undefined,
      verificationState: state || undefined,
      attentionOnly,
      codeOnly,
      commitOnly,
      page,
      pageSize: PAGE_SIZE,
    }),
  });
  const detail = useQuery({ queryKey: ["session", selectedId], queryFn: () => api.sessionDetail(selectedId ?? ""), enabled: Boolean(selectedId) });

  useEffect(() => {
    void api.refreshIndex(false);
  }, []);

  useEffect(() => {
    if (!query.data) return;
    setItems((current) => (query.data.page === 0 ? query.data.items : [...current, ...query.data.items.filter((item) => !current.some((existing) => existing.id === item.id))]));
  }, [query.data]);

  const models = query.data?.models ?? [];
  const projects = query.data?.projects ?? [];
  const total = query.data?.total ?? 0;
  const hasMore = items.length < total;
  const loadingMore = query.isFetching && page > 0;

  return (
    <div className={`page sessions-page ${selectedId ? "showing-replay" : ""} ${embedded ? "embedded" : ""}`}>
      {!selectedId ? <>
        {embedded ? (
          <header className="sessions-embedded-header">
            <button className="button subtle" onClick={onBack}><ArrowLeft size={14} />{t("data.backToOverview")}</button>
            <div>
              <span className="eyebrow">{t("sessions.title")}</span>
              <h1>{t("sessions.ledgerTitle")}</h1>
              <p>{t("sessions.description")}</p>
            </div>
            <RangePicker />
          </header>
        ) : (
          <PageHeader title={t("sessions.title")} description={t("sessions.description")} actions={<RangePicker />} />
        )}
        <div className="session-toolbar">
          <label className="search-field"><Search size={16} /><input value={searchInput} onChange={(event) => setSearchInput(event.target.value)} placeholder={t("sessions.search")} />{searchInput ? <button onClick={() => setSearchInput("")} aria-label={t("actions.clear")}><X size={14} /></button> : null}</label>
        <select value={agent} onChange={(event) => setAgent(event.target.value)}><option value="">{t("sessions.allAgents")}</option><option value="claude-code">Claude Code</option><option value="codex">Codex</option><option value="kimi-code">Kimi Code</option><option value="cursor">Cursor</option><option value="openclaw">OpenClaw</option><option value="hermes">Hermes</option><option value="zcode">ZCode</option></select>
          <select value={model} onChange={(event) => setModel(event.target.value)}><option value="">{t("sessions.allModels")}</option>{models.map((item) => <option key={item} value={item}>{item}</option>)}</select>
          <select value={project} onChange={(event) => setProject(event.target.value)}><option value="">{t("sessions.allProjects")}</option>{projects.map((item) => <option key={item} value={item}>{item}</option>)}</select>
          <select value={state} onChange={(event) => setState(event.target.value)}><option value="">{t("sessions.allStates")}</option><option value="verified">{t("sessions.verification.verified")}</option><option value="unverified">{t("sessions.verification.unverified")}</option><option value="not-applicable">{t("sessions.verification.not-applicable")}</option></select>
          <label className="filter-check"><input type="checkbox" checked={attentionOnly} onChange={(event) => setAttentionOnly(event.target.checked)} />{t("sessions.attentionOnly")}</label>
          <label className="filter-check"><input type="checkbox" checked={codeOnly} onChange={(event) => setCodeOnly(event.target.checked)} />{t("sessions.codeOnly")}</label>
          <label className="filter-check"><input type="checkbox" checked={commitOnly} onChange={(event) => setCommitOnly(event.target.checked)} />{t("sessions.commitOnly")}</label>
          <span className="result-count">{t("sessions.resultCount", { count: total, shown: items.length })}</span>
        </div>
        <section className="session-ledger">
          {query.isLoading && page === 0 ? <LoadingState /> : query.isError ? <ErrorState retry={() => void query.refetch()} /> : items.length === 0 ? <EmptyState title={t("sessions.emptyTitle")} body={t("sessions.emptyBody")} /> : items.map((session) => (
            <button className="session-ledger-row" key={session.id} onClick={() => selectSession(session.id)}>
              <span className="session-date">{formatDateTime(session.startedAt, locale)}</span>
              <AgentBadge agent={session.agent} model={session.model} />
              <span className="session-copy"><strong><SessionTitle session={session} /></strong><small>{session.projectLabel}</small></span>
              <SessionEvidence session={session} locale={locale} />
              <span className="session-usage"><strong>{formatCompact(tokenTotal(session.usage), locale)}</strong><small>{t("metrics.tokens")}</small></span>
              <VerificationPill value={session.verificationState} />
              <ChevronRight size={15} />
            </button>
          ))}
        </section>
        {hasMore ? (
          <div className="session-pagination">
            <button className="button secondary" disabled={loadingMore} onClick={() => setPage((current) => current + 1)}>
              {loadingMore ? t("actions.refreshing") : t("sessions.loadMore")}
            </button>
          </div>
        ) : null}
      </> : detail.isLoading ? <LoadingState /> : detail.isError || !detail.data ? <ErrorState retry={() => void detail.refetch()} /> : <SessionReplay detail={detail.data} locale={locale} onClose={() => selectSession(undefined)} />}
    </div>
  );
}
