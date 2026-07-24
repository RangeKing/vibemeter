import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, ArrowRight, CheckCircle2, CircleDashed, GitCommitHorizontal, Merge, Sparkles } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { EmptyState, ErrorState, LoadingState } from "../components/ui";
import { api } from "../lib/api";
import { formatCompact, formatDate, formatDuration, formatPercent, tokenTotal } from "../lib/format";
import { useUiStore } from "../store";
import type { Locale, TaskSummary } from "../types";

function TaskCard({ task, locale, compact = false, selected = false, onSelect, onOpen }: { task: TaskSummary; locale: Locale; compact?: boolean; selected?: boolean; onSelect?: (value: boolean) => void; onOpen?: () => void }) {
  const { t } = useTranslation();
  const state = task.hasCommit || task.verificationState === "verified" ? "verified" : task.status;
  const StateIcon = state === "verified" ? CheckCircle2 : task.worthReviewing ? AlertTriangle : CircleDashed;
  return (
    <article className={`task-card ${compact ? "compact" : ""} ${task.worthReviewing ? "needs-review" : ""}`}>
      <header>
        <span className="task-header-left">{onSelect ? <input type="checkbox" checked={selected} onChange={(event) => onSelect(event.target.checked)} aria-label={t("task.selected", { count: selected ? 1 : 0 })} /> : null}<span className={`task-state ${state}`}><StateIcon size={14} />{t(`task.${state in { verified: 1, changed: 1, blocked: 1 } ? state : "unverified"}`)}</span></span>
        <span className="task-time">{formatDate(task.startedAt, locale, "short")}</span>
      </header>
      <h3>{task.title}</h3>
      <p className="task-project">{task.projectLabel || "—"}</p>
      <div className="task-evidence-row">
        <span><strong>{formatCompact(task.filesChanged, locale)}</strong>{t("metrics.files")}</span>
        <span><strong>+{formatCompact(task.linesAdded, locale)} / −{formatCompact(task.linesDeleted, locale)}</strong>{t("metrics.lines")}</span>
        {task.hasCommit ? <span className="commit-evidence"><GitCommitHorizontal size={13} />{t("task.commitObserved")}</span> : null}
      </div>
      {task.reviewReasonKeys.length ? <div className="reason-list">{task.reviewReasonKeys.map((key) => <span key={key}>{t(key)}</span>)}</div> : null}
      <footer><span>{compact ? t("task.sessions", { count: task.sessionCount }) : t("task.autoGrouped", { value: formatPercent(task.confidence, locale) })}</span>{onOpen && task.primarySessionId ? <button className="inline-link" onClick={onOpen}>{t("actions.viewEvidence")}<ArrowRight size={12} /></button> : null}</footer>
    </article>
  );
}

export function TodayPage({ locale }: { locale: Locale }) {
  const { t } = useTranslation();
  const client = useQueryClient();
  const setPage = useUiStore((state) => state.setPage);
  const setRange = useUiStore((state) => state.setRange);
  const selectSession = useUiStore((state) => state.selectSession);
  const [selectedTasks, setSelectedTasks] = useState<string[]>([]);
  const query = useQuery({ queryKey: ["today"], queryFn: api.today, refetchInterval: 30_000 });
  const openSessions = () => { setRange("today"); setPage("sessions"); };
  const openTask = (task: TaskSummary) => { if (task.primarySessionId) selectSession(task.primarySessionId); setRange("today"); setPage("sessions"); };
  const merge = useMutation({ mutationFn: () => api.mergeTasks(selectedTasks), onSuccess: async () => { setSelectedTasks([]); await Promise.all([client.invalidateQueries({ queryKey: ["today"] }), client.invalidateQueries({ queryKey: ["sessions"] })]); } });
  const toggleTask = (id: string, selected: boolean) => setSelectedTasks((current) => selected ? [...new Set([...current, id])] : current.filter((item) => item !== id));

  if (query.isLoading) return <LoadingState />;
  if (query.isError || !query.data) return <ErrorState retry={() => void query.refetch()} />;
  const data = query.data;
  const verified = data.tasks.filter((task) => task.hasCommit || task.verificationState === "verified").length;
  const primaryInsight = data.insights[0];

  return (
    <div className="page today-page">
      <section className="today-hero">
        <div className="hero-copy">
          <span className="eyebrow"><Sparkles size={13} />{t("today.eyebrow")} · {formatDate(data.date, locale)}</span>
          <h1>{t("today.title")}</h1>
          <p>{t("today.description")}</p>
          {primaryInsight ? <div className={`hero-insight ${primaryInsight.tier}`}><span />{t(primaryInsight.messageKey)}</div> : null}
        </div>
        <div className="hero-ledger" aria-label={t("today.secondary")}>
          <div className="ledger-primary"><span>{t("metrics.tasks")}</span><strong>{data.tasks.length}</strong><small>{verified} {t("task.verified").toLowerCase()}</small></div>
          <div><span>{t("metrics.files")}</span><strong>{formatCompact(data.totals.filesTouched, locale)}</strong></div>
          <div><span>{t("metrics.duration")}</span><strong>{formatDuration(data.totals.activeSeconds, locale)}</strong></div>
          <div><span>{t("metrics.tokens")}</span><strong>{formatCompact(tokenTotal(data.totals.usage), locale)}</strong></div>
        </div>
      </section>

      <div className="today-grid">
        <section className="today-thread">
          <header className="section-heading">
            <div><span className="section-index">01</span><h2>{t("today.tasks")}</h2><p>{t("today.tasksBody")}</p></div>
            <button className="inline-link" onClick={openSessions}>{t("actions.viewEvidence")}<ArrowRight size={14} /></button>
          </header>
          {selectedTasks.length ? <div className="task-merge-bar"><span>{t("task.selected", { count: selectedTasks.length })}</span><button className="button secondary" onClick={() => merge.mutate()} disabled={selectedTasks.length < 2 || merge.isPending}><Merge size={13} />{t("task.merge")}</button></div> : null}
          {data.tasks.length ? <div className="task-grid">{data.tasks.map((task) => <TaskCard key={task.id} task={task} locale={locale} selected={selectedTasks.includes(task.id)} onSelect={(value) => toggleTask(task.id, value)} onOpen={() => openTask(task)} />)}</div> : <EmptyState title={t("today.emptyTitle")} body={t("today.emptyBody")} />}
        </section>

        <aside className="review-rail">
          <header className="section-heading compact-heading">
            <div><span className="section-index">02</span><h2>{t("today.worthReviewing")}</h2><p>{t("today.worthBody")}</p></div>
          </header>
          {data.worthReviewing.length ? <div className="review-stack">{data.worthReviewing.map((task) => <TaskCard key={task.id} task={task} locale={locale} compact onOpen={() => openTask(task)} />)}</div> : <div className="quiet-empty"><CheckCircle2 size={22} /><span>{t("today.noReview")}</span></div>}
          <button className="button secondary full" onClick={() => setPage("reviews")}>{t("actions.generate")}<ArrowRight size={14} /></button>
        </aside>
      </div>
    </div>
  );
}
