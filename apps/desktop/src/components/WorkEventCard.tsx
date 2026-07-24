import { AlertTriangle, ArrowRight, CheckCircle2, CircleDashed, GitCommitHorizontal, Merge } from "lucide-react";
import { useTranslation } from "react-i18next";
import { formatCompact, formatDate, formatPercent } from "../lib/format";
import type { Locale, TaskSummary } from "../types";

export function WorkEventCard({ task, locale, selected = false, onSelect, onOpen, onAcceptSuggestion }: {
  task: TaskSummary;
  locale: Locale;
  selected?: boolean;
  onSelect?: (value: boolean) => void;
  onOpen?: () => void;
  onAcceptSuggestion?: () => void;
}) {
  const { t } = useTranslation();
  const state = task.hasCommit || task.verificationState === "verified" ? "verified" : task.status;
  const StateIcon = state === "verified" ? CheckCircle2 : task.worthReviewing ? AlertTriangle : CircleDashed;
  const grouping = task.groupingState === "suggested"
    ? t("task.suggestedGrouping", { value: formatPercent(task.confidence, locale) })
    : task.sessionCount > 1
      ? t(task.groupingState === "manual" ? "task.manuallyGrouped" : "task.groupedSessions", { count: task.sessionCount, value: formatPercent(task.confidence, locale) })
      : t("task.singleSession");

  return (
    <article className={`task-card event-card ${task.worthReviewing ? "needs-review" : ""}`}>
      <header>
        <span className="task-header-left">
          {onSelect ? <input type="checkbox" checked={selected} onChange={(event) => onSelect(event.target.checked)} aria-label={t("task.selected", { count: selected ? 1 : 0 })} /> : null}
          <span className={`task-state ${state}`}><StateIcon size={14} />{t(`task.${state in { verified: 1, changed: 1, blocked: 1 } ? state : "unverified"}`)}</span>
        </span>
        <span className="task-time">{formatDate(task.startedAt, locale, "short")}</span>
      </header>
      <h3>{task.title || task.projectLabel}</h3>
      <p className="task-project">{task.projectLabel || "—"}</p>
      <div className="task-evidence-row">
        <span><strong>{formatCompact(task.filesChanged, locale)}</strong>{t("metrics.files")}</span>
        <span><strong>+{formatCompact(task.linesAdded, locale)} / −{formatCompact(task.linesDeleted, locale)}</strong>{t("metrics.lines")}</span>
        {task.hasCommit ? <span className="commit-evidence"><GitCommitHorizontal size={13} />{t("task.commitObserved")}</span> : null}
      </div>
      {task.reviewReasonKeys.length ? <div className="reason-list">{task.reviewReasonKeys.slice(0, 2).map((key) => <span key={key}>{t(key)}</span>)}</div> : null}
      {task.groupingReasonKeys.length ? <div className="grouping-reasons">{task.groupingReasonKeys.map((key) => <span key={key}>{t(key)}</span>)}</div> : null}
      <footer><span>{grouping}</span><span className="event-card-actions">{onAcceptSuggestion && task.suggestedTaskId ? <button className="inline-link suggestion-action" onClick={onAcceptSuggestion}><Merge size={12} />{t("task.acceptSuggestion")}</button> : null}{onOpen && task.primarySessionId ? <button className="inline-link" onClick={onOpen}>{t("actions.viewEvidence")}<ArrowRight size={12} /></button> : null}</span></footer>
    </article>
  );
}
