import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowLeft, CheckCircle2, ChevronDown, ChevronRight, FileCode2, GitBranch, GitCommitHorizontal, Search, Split, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { FocusEvent as ReactFocusEvent, MouseEvent as ReactMouseEvent } from "react";
import { useTranslation } from "react-i18next";
import { RangePicker } from "./RangePicker";
import { AgentBadge, EmptyState, ErrorState, LoadingState, PageHeader, SessionEvidence, SessionTitle, VerificationPill } from "./ui";
import { api } from "../lib/api";
import { formatCompact, formatDateTime, formatDuration, tokenTotal } from "../lib/format";
import { useUiStore } from "../store";
import type { Locale, SessionDetail, SessionSummary } from "../types";
import { buildTrajectory, type TrajectoryLane } from "./sessionTrajectory";

const PAGE_SIZE = 50;
const phaseKeys = new Set(["understand", "inspect", "edit", "verify", "fix", "plan", "execute"]);
type TrajectoryTooltip = { text: string; x: number; y: number };

function formatPhaseTime(value: string | undefined, locale: Locale): string | undefined {
  if (!value) return undefined;
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return undefined;
  return new Intl.DateTimeFormat(locale, { hour: "2-digit", minute: "2-digit" }).format(date);
}

function formatTrajectoryOffset(milliseconds: number, totalMilliseconds: number): string {
  if (totalMilliseconds < 10_000) {
    const seconds = milliseconds / 1_000;
    return `${seconds.toFixed(Number.isInteger(seconds) ? 0 : 1)}s`;
  }
  const totalSeconds = Math.round(milliseconds / 1_000);
  const hours = Math.floor(totalSeconds / 3_600);
  const minutes = Math.floor((totalSeconds % 3_600) / 60);
  const seconds = totalSeconds % 60;
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`
    : `${minutes}:${String(seconds).padStart(2, "0")}`;
}

export function SessionReplay({ detail, locale, onClose }: { detail: SessionDetail; locale: Locale; onClose: () => void }) {
  const { t } = useTranslation();
  const client = useQueryClient();
  const [showAllPhases, setShowAllPhases] = useState(false);
  const [expandedPhaseIds, setExpandedPhaseIds] = useState<Set<string>>(() => new Set());
  const [activePhaseId, setActivePhaseId] = useState<string>();
  const [trajectoryTooltip, setTrajectoryTooltip] = useState<TrajectoryTooltip>();
  const phaseRefs = useRef(new Map<string, HTMLElement>());
  const highlightTimer = useRef<number | undefined>(undefined);
  const visiblePhases = showAllPhases ? detail.phases : detail.phases.slice(0, 24);
  const trajectory = useMemo(() => buildTrajectory(detail.phases, {
    startedAt: detail.startedAt,
    endedAt: detail.endedAt,
  }), [detail.endedAt, detail.phases, detail.startedAt]);
  const laneSpans = useMemo(() => ({
    input: trajectory.spans.filter((span) => span.lane === "input"),
    agent: trajectory.spans.filter((span) => span.lane === "agent"),
    tools: trajectory.spans.filter((span) => span.lane === "tools"),
  }), [trajectory.spans]);
  const totalEvents = detail.phases.reduce((sum, phase) => sum + phase.events.length, 0);
  const sequenceTickCount = Math.min(5, Math.max(2, totalEvents));
  const phaseRailGap = detail.phases.length > 120 ? 0 : detail.phases.length > 60 ? 1 : 2;
  const axisTicks = trajectory.scale === "time"
    ? [0, 0.25, 0.5, 0.75, 1]
    : Array.from({ length: sequenceTickCount }, (_, index) => index / (sequenceTickCount - 1));

  useEffect(() => {
    if (highlightTimer.current) window.clearTimeout(highlightTimer.current);
    setShowAllPhases(false);
    setExpandedPhaseIds(new Set());
    setActivePhaseId(undefined);
    setTrajectoryTooltip(undefined);
  }, [detail.id]);

  useEffect(() => () => {
    if (highlightTimer.current) window.clearTimeout(highlightTimer.current);
  }, []);

  function togglePhase(phaseId: string) {
    setExpandedPhaseIds((current) => {
      const next = new Set(current);
      if (next.has(phaseId)) next.delete(phaseId);
      else next.add(phaseId);
      return next;
    });
  }

  function focusPhase(phaseId: string, revealEvents = false) {
    if (revealEvents) setExpandedPhaseIds((current) => new Set(current).add(phaseId));
    if (!detail.phases.slice(0, 24).some((phase) => phase.id === phaseId)) setShowAllPhases(true);
    setActivePhaseId(phaseId);
    if (highlightTimer.current) window.clearTimeout(highlightTimer.current);
    window.requestAnimationFrame(() => {
      const reduceMotion = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
      phaseRefs.current.get(phaseId)?.scrollIntoView({ behavior: reduceMotion ? "auto" : "smooth", block: "center" });
      highlightTimer.current = window.setTimeout(() => setActivePhaseId(undefined), 900);
    });
  }

  function phaseLabel(phaseKey: string) {
    return t(`sessions.phase.${phaseKeys.has(phaseKey) ? phaseKey : "other"}`);
  }

  function showTrajectoryTooltip(text: string, x: number, y: number) {
    const horizontalMargin = 112;
    setTrajectoryTooltip({
      text,
      x: Math.min(Math.max(x, horizontalMargin), Math.max(horizontalMargin, window.innerWidth - horizontalMargin)),
      y: Math.max(32, y - 8),
    });
  }

  function showMouseTooltip(event: ReactMouseEvent<HTMLElement>, text: string) {
    showTrajectoryTooltip(text, event.clientX, event.clientY);
  }

  function showFocusTooltip(event: ReactFocusEvent<HTMLElement>, text: string) {
    const bounds = event.currentTarget.getBoundingClientRect();
    showTrajectoryTooltip(text, bounds.left + bounds.width / 2, bounds.top);
  }

  function eventStatus(success: boolean | undefined) {
    if (success === true) return "success";
    if (success === false) return "failed";
    return "unknown";
  }

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
        <header><div><h3>{t("sessions.process")}</h3><p>{t("sessions.processBody")}</p></div><span>{t("sessions.eventCount", { count: totalEvents })}</span></header>
        {detail.phases.length ? (
          <div className="trajectory-overview" aria-label={t("sessions.trajectoryOverview")}>
            <div className="trajectory-overview-head">
              <span>{t("sessions.phaseOverview")}</span>
              <strong>{t(`sessions.scale.${trajectory.scale}`)}</strong>
            </div>
            <div className="trajectory-phase-rail" style={{ gap: `${phaseRailGap}px` }}>
              {detail.phases.map((phase) => {
                const failures = phase.events.filter((event) => event.success === false);
                const successes = phase.events.filter((event) => event.success === true).length;
                const title = [
                  phaseLabel(phase.phaseKey),
                  t("sessions.eventCount", { count: phase.eventCount }),
                  successes ? t("sessions.successful", { count: successes }) : undefined,
                  failures.length ? t("sessions.failed", { count: failures.length }) : undefined,
                ].filter(Boolean).join(" · ");
                return (
                  <button
                    key={phase.id}
                    className={`trajectory-phase-segment phase-${phaseKeys.has(phase.phaseKey) ? phase.phaseKey : "other"}`}
                    style={{ flexGrow: Math.max(1, phase.eventCount), flexShrink: 1, flexBasis: 0, minWidth: 0 }}
                    onClick={() => focusPhase(phase.id)}
                    onMouseEnter={(event) => showMouseTooltip(event, title)}
                    onMouseLeave={() => setTrajectoryTooltip(undefined)}
                    onFocus={(event) => showFocusTooltip(event, title)}
                    onBlur={() => setTrajectoryTooltip(undefined)}
                    aria-label={title}
                  >
                    {failures.map((event, index) => <i key={`failure-${event.sequence}-${index}`} className="trajectory-failure-tick" style={{ left: `${phase.events.length > 1 ? (phase.events.indexOf(event) / (phase.events.length - 1)) * 100 : 50}%` }} />)}
                  </button>
                );
              })}
            </div>
            <div className="trajectory-lanes">
              {(["input", "agent", "tools"] as TrajectoryLane[]).map((lane) => (
                <div className={`trajectory-lane lane-${lane}`} key={lane}>
                  <span>{t(`sessions.lane.${lane}`)}</span>
                  <div className="trajectory-lane-track">
                    {laneSpans[lane].map((span, index) => {
                      const time = formatPhaseTime(span.event.occurredAt, locale);
                      const status = eventStatus(span.event.success);
                      const duration = span.durationMs && span.durationMs > 0
                        ? formatTrajectoryOffset(span.durationMs, trajectory.durationMs ?? span.durationMs)
                        : undefined;
                      const title = [span.event.name, time, duration].filter(Boolean).join(" · ");
                      return (
                        <button
                          key={`${lane}-${span.phaseId}-${span.event.sequence}-${index}`}
                          className={`trajectory-span ${span.instant ? "is-instant" : ""} status-${status}`}
                          style={{ left: `${span.position}%`, width: `${span.width}%` }}
                          onClick={() => focusPhase(span.phaseId, true)}
                          onMouseEnter={(event) => showMouseTooltip(event, title)}
                          onMouseLeave={() => setTrajectoryTooltip(undefined)}
                          onFocus={(event) => showFocusTooltip(event, title)}
                          onBlur={() => setTrajectoryTooltip(undefined)}
                          aria-label={`${t(`sessions.lane.${lane}`)} · ${title}`}
                        />
                      );
                    })}
                  </div>
                </div>
              ))}
            </div>
            <div className="trajectory-time-axis" aria-label={t("sessions.timelineAxis")}>
              <span aria-hidden="true" />
              <div>
                {axisTicks.map((tick, index) => (
                  <time
                    key={`${trajectory.scale}-${tick}`}
                    className={index === 0 ? "is-first" : index === axisTicks.length - 1 ? "is-last" : ""}
                    style={{ left: `${tick * 100}%` }}
                  >
                    {trajectory.scale === "time" && trajectory.durationMs !== null
                      ? formatTrajectoryOffset(trajectory.durationMs * tick, trajectory.durationMs)
                      : String(Math.max(1, Math.round(1 + tick * (totalEvents - 1))))}
                  </time>
                ))}
              </div>
            </div>
            {trajectoryTooltip ? (
              <div
                className="trajectory-hover-tooltip"
                role="tooltip"
                style={{ left: trajectoryTooltip.x, top: trajectoryTooltip.y }}
              >
                {trajectoryTooltip.text}
              </div>
            ) : null}
          </div>
        ) : null}
        <div className="phase-timeline">
          {visiblePhases.map((phase, index) => {
            const failures = phase.events.filter((event) => event.success === false).length;
            const successes = phase.events.filter((event) => event.success === true).length;
            const durationMs = phase.events.reduce((sum, event) => sum + (event.durationMs ?? 0), 0);
            const phaseTime = formatPhaseTime(phase.startedAt, locale);
            const expanded = expandedPhaseIds.has(phase.id);
            const visibleEvents = expanded ? phase.events : phase.events.slice(0, 5);
            const phaseClass = phaseKeys.has(phase.phaseKey) ? phase.phaseKey : "other";
            return <article
              key={phase.id}
              ref={(node) => { if (node) phaseRefs.current.set(phase.id, node); else phaseRefs.current.delete(phase.id); }}
              className={`phase phase-${phaseClass} ${activePhaseId === phase.id ? "is-active" : ""}`}
            >
              <div className="phase-axis"><span>{String(index + 1).padStart(2, "0")}</span></div>
              <div className="phase-body">
                <button className="phase-toggle" onClick={() => togglePhase(phase.id)} aria-expanded={expanded}>
                  <span className="phase-title"><strong>{phaseLabel(phase.phaseKey)}</strong><small>{t("sessions.eventCount", { count: phase.eventCount })}</small></span>
                  <span className="phase-meta">{phaseTime ? <span>{phaseTime}</span> : null}{durationMs ? <span>{formatDuration(Math.max(1, Math.round(durationMs / 1_000)), locale)}</span> : null}{successes ? <span className="successful">{t("sessions.successful", { count: successes })}</span> : null}{failures ? <span className="failed">{t("sessions.failed", { count: failures })}</span> : null}<ChevronDown size={14} /></span>
                </button>
                <div className="phase-event-list">
                  {visibleEvents.map((event, eventIndex) => {
                    const status = eventStatus(event.success);
                    const eventTime = formatPhaseTime(event.occurredAt, locale);
                    const contentPreview = event.eventType === "prompt.observed"
                      ? { label: t("sessions.promptPreview"), text: detail.contentPreview.prompt }
                      : event.eventType === "lifecycle.complete"
                        ? { label: t("sessions.outputPreview"), text: detail.contentPreview.output }
                        : undefined;
                    return (
                      <div className={`phase-event-row status-${status}`} key={`${event.sequence}-${event.name}-${eventIndex}`}>
                        <i aria-hidden="true" />
                        <span className="phase-event-copy"><strong>{event.name}</strong><small>{event.eventType}</small></span>
                        {contentPreview?.text ? (
                          <details className="phase-event-content">
                            <summary><small>{contentPreview.label}</small><span>{contentPreview.text}</span></summary>
                            <p>{contentPreview.text}</p>
                          </details>
                        ) : <span />}
                        <span className="phase-event-meta">
                          <small className="phase-event-status">{t(`sessions.status.${status}`)}</small>
                          {eventTime ? <time>{eventTime}</time> : null}
                          {event.durationMs ? <time>{formatDuration(Math.max(1, Math.round(event.durationMs / 1_000)), locale)}</time> : null}
                        </span>
                      </div>
                    );
                  })}
                </div>
                {phase.events.length > 5 ? (
                  <button className="phase-events-toggle" onClick={() => togglePhase(phase.id)} aria-expanded={expanded}>
                    {expanded ? t("sessions.collapsePhaseEvents") : t("sessions.expandPhaseEvents", { count: phase.events.length - 5 })}
                    <ChevronDown size={13} />
                  </button>
                ) : null}
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
}: {
  locale: Locale;
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
    <div className={`page sessions-page ${selectedId ? "showing-replay" : ""}`}>
      {!selectedId ? <>
        <PageHeader title={t("sessions.title")} description={t("sessions.description")} actions={<RangePicker />} />
        <div className="session-toolbar">
          <label className="search-field"><Search size={16} /><input value={searchInput} onChange={(event) => setSearchInput(event.target.value)} placeholder={t("sessions.search")} />{searchInput ? <button onClick={() => setSearchInput("")} aria-label={t("actions.clear")}><X size={14} /></button> : null}</label>
        <select value={agent} onChange={(event) => setAgent(event.target.value)}><option value="">{t("sessions.allAgents")}</option><option value="claude-code">Claude Code</option><option value="codex">Codex</option><option value="deepseek-harness">DeepSeek Harness</option><option value="kimi-code">Kimi Code</option><option value="cursor">Cursor</option><option value="openclaw">OpenClaw</option><option value="hermes">Hermes</option><option value="zcode">ZCode</option></select>
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
