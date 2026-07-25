import * as Switch from "@radix-ui/react-switch";
import { AlertCircle, Check, Clock3, Database, GitCommitHorizontal, LoaderCircle } from "lucide-react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { agentName, formatCompact, formatDuration, tokenTotal } from "../lib/format";
import type { Locale, OverviewTotals, SessionSummary } from "../types";

export function PageHeader({ title, description, actions }: { title: string; description: string; actions?: ReactNode }) {
  return (
    <header className="page-header">
      <div>
        <h1>{title}</h1>
        <p>{description}</p>
      </div>
      {actions ? <div className="page-actions">{actions}</div> : null}
    </header>
  );
}

export function LoadingState({ compact = false }: { compact?: boolean }) {
  return (
    <div className={compact ? "state state-compact" : "state"} aria-live="polite">
      <LoaderCircle className="spin" size={compact ? 17 : 24} />
    </div>
  );
}

export function ErrorState({ retry }: { retry?: () => void }) {
  const { t } = useTranslation();
  return (
    <div className="state state-message" role="alert">
      <span className="state-icon warning"><AlertCircle size={22} /></span>
      <h2>{t("errors.genericTitle")}</h2>
      <p>{t("errors.genericBody")}</p>
      {retry ? <button className="button secondary" onClick={retry}>{t("actions.retry")}</button> : null}
    </div>
  );
}

export function EmptyState({ title, body }: { title: string; body: string }) {
  return (
    <div className="state state-message">
      <span className="state-icon"><Database size={22} /></span>
      <h2>{title}</h2>
      <p>{body}</p>
    </div>
  );
}

export function Toggle({ checked, onCheckedChange, label, disabled = false }: { checked: boolean; onCheckedChange: (value: boolean) => void; label: string; disabled?: boolean }) {
  return (
    <Switch.Root className="switch-root" checked={checked} onCheckedChange={onCheckedChange} aria-label={label} disabled={disabled}>
      <Switch.Thumb className="switch-thumb" />
    </Switch.Root>
  );
}

export function AgentBadge({ agent, model, compact = false }: { agent: string; model?: string; compact?: boolean }) {
  const glyph = agent === "claude-code" ? "C" : agent === "codex" ? "O" : agent === "kimi-code" ? "K" : agent === "cursor" ? "↗" : agent === "openclaw" ? "◌" : agent === "hermes" ? "H" : "A";
  const kind = agent === "claude-code" ? "claude" : agent === "codex" ? "codex" : agent === "kimi-code" ? "kimi" : agent === "cursor" ? "cursor" : agent === "openclaw" ? "openclaw" : agent === "hermes" ? "hermes" : "vibemeter";
  return (
    <span className={compact ? "agent-badge compact" : "agent-badge"}>
      <span className={`agent-glyph ${kind}`}>{glyph}</span>
      <span>
        <strong>{agentName(agent)}</strong>
        {!compact && model ? <small title={model}>{model}</small> : null}
      </span>
    </span>
  );
}

export function MetricStrip({ totals, locale }: { totals: OverviewTotals; locale: Locale }) {
  const { t } = useTranslation();
  const values = [
    [t("metrics.sessions"), formatCompact(totals.sessionCount, locale)],
    [t("metrics.duration"), formatDuration(totals.activeSeconds, locale)],
    [t("metrics.tokens"), formatCompact(tokenTotal(totals.usage), locale)],
    [t("metrics.activeDays"), formatCompact(totals.activeDays, locale)],
  ];
  return (
    <div className="metric-strip">
      {values.map(([label, value]) => (
        <div className="metric-item" key={String(label)}>
          <span>{label}</span>
          <strong>{value}</strong>
        </div>
      ))}
    </div>
  );
}

export function VerificationPill({ value }: { value: string }) {
  const { t } = useTranslation();
  const verified = value === "verified";
  const key = value === "verified" || value === "unverified" || value === "not-applicable" ? value : "not-applicable";
  return (
    <span className={`verification ${verified ? "verified" : "neutral"}`}>
      {verified ? <Check size={13} /> : <Clock3 size={13} />}
      {t(`sessions.verification.${key}`)}
    </span>
  );
}

export function SessionEvidence({ session, locale }: { session: SessionSummary; locale: Locale }) {
  const { t } = useTranslation();
  const hasLineEvidence = session.linesAdded + session.linesDeleted > 0;
  const hasFileEvidence = session.filesTouched > 0;
  const verified = session.verificationState === "verified";
  return (
    <span className={`session-evidence ${verified ? "verified" : ""}`}>
      <strong>
        {hasLineEvidence
          ? `+${formatCompact(session.linesAdded, locale)} / −${formatCompact(session.linesDeleted, locale)}`
          : formatCompact(hasFileEvidence ? session.filesTouched : session.toolCalls, locale)}
      </strong>
      <small>
        {session.hasCommit ? <GitCommitHorizontal size={11} aria-hidden="true" /> : verified ? <Check size={11} aria-hidden="true" /> : null}
        {session.hasCommit ? t("task.commitObserved") : verified ? t("sessions.verification.verified") : t(hasLineEvidence ? "metrics.lines" : hasFileEvidence ? "metrics.files" : "metrics.tools")}
      </small>
    </span>
  );
}

export function SessionTitle({ session }: { session: SessionSummary }) {
  const { t } = useTranslation();
  return <>{session.title.trim() || t("sessions.untitled")}</>;
}
