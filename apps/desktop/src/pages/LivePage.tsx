import { useQuery } from "@tanstack/react-query";
import type { TFunction } from "i18next";
import { useState } from "react";
import {
  ArrowRight,
  ArrowUpRight,
  Check,
  CircleAlert,
  CirclePause,
  Clock3,
  RadioTower,
  Settings2,
  ShieldCheck,
  Waves,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { AgentBadge, EmptyState, ErrorState, LoadingState } from "../components/ui";
import { api } from "../lib/api";
import { agentName, formatDateTime, formatTime } from "../lib/format";
import { sourceCapabilityNameGroups } from "../lib/sourceStatus";
import { useLiveSnapshot } from "../lib/useLiveSnapshot";
import { useUiStore } from "../store";
import type {
  AttentionEvent,
  AttentionQualityReport,
  LiveHistoryItem,
  LiveSession,
  LiveTimelinePoint,
  Locale,
  WorkPulseDimension,
} from "../types";

type AttentionFeedback = "handled" | "not-relevant" | "not-stuck" | "snoozed";

const attentionPriority: Record<AttentionEvent["kind"], number> = {
  waiting: 0,
  error: 1,
  stuck: 2,
  "completion-review": 3,
};

export function sortAttentionEvents(items: AttentionEvent[]): AttentionEvent[] {
  return [...items].sort((left, right) =>
    attentionPriority[left.kind] - attentionPriority[right.kind]
    || left.openedAt.localeCompare(right.openedAt)
    || left.id.localeCompare(right.id));
}

export function AttentionActions({
  kind,
  onFeedback,
}: {
  kind: AttentionEvent["kind"];
  onFeedback: (feedback: AttentionFeedback) => void;
}) {
  const { t } = useTranslation();
  const choices: AttentionFeedback[] = kind === "stuck"
    ? ["handled", "not-relevant", "not-stuck", "snoozed"]
    : ["handled", "not-relevant", "snoozed"];
  return (
    <div className="attention-actions">
      {choices.map((choice) => (
        <button key={choice} className="button subtle" onClick={() => onFeedback(choice)}>
          {t(`live.attention.action.${choice}`)}
        </button>
      ))}
    </div>
  );
}

export function AttentionQueue({
  items,
  jumpErrors = new Set(),
  onFeedback,
  onJump,
}: {
  items: AttentionEvent[];
  jumpErrors?: ReadonlySet<string>;
  onFeedback: (id: string, feedback: AttentionFeedback) => void;
  onJump: (id: string) => void;
}) {
  const { t } = useTranslation();
  const visible = sortAttentionEvents(items);
  if (!visible.length) {
    return <div className="attention-empty">{t("live.attention.empty")}</div>;
  }
  return (
    <div className="attention-queue">
      {visible.map((attention) => (
        <article key={attention.id} className={`kind-${attention.kind}`}>
          <header>
            <div>
              <strong>{t(`live.attention.kind.${attention.kind}`)}</strong>
              <small>{attention.projectLabel || agentName(attention.agent)}</small>
            </div>
            <span>{t(`live.attention.state.${attention.state}`)}</span>
          </header>
          <p>{t(`live.attention.reason.${attention.reasonKey}`, { defaultValue: attention.reasonKey })}</p>
          <small>{t("live.attention.evidence", { count: attention.evidenceCount })}</small>
          {jumpErrors.has(attention.id) ? (
            <p className="attention-jump-error" role="alert">{t("live.attention.jumpFailed")}</p>
          ) : null}
          <footer>
            <AttentionActions
              kind={attention.kind}
              onFeedback={(feedback) => onFeedback(attention.id, feedback)}
            />
            <button className="button secondary" onClick={() => onJump(attention.id)}>
              {t("live.jump")}<ArrowUpRight size={13} />
            </button>
          </footer>
        </article>
      ))}
    </div>
  );
}

function percentage(value: number | null): string | null {
  return value === null ? null : `${Math.round(value * 1_000) / 10}%`;
}

export function AttentionQualityGate({ report }: { report: AttentionQualityReport }) {
  const { t } = useTranslation();
  const unavailable = t("live.pulse.notRecorded");
  const stuckPrecision = percentage(report.stuckPrecision);
  const falsePositiveRate = percentage(report.falsePositiveRate);
  const jumpSuccessRate = percentage(report.jumpSuccessRate);
  return (
    <aside className={`attention-quality state-${report.passed ? "passed" : "incomplete"}`}>
      <header>
        <div>
          <strong>{t("live.attention.qualityTitle")}</strong>
          <small>{t("live.attention.qualityBody")}</small>
        </div>
        <span>{t(report.passed ? "live.attention.qualityPassed" : "live.attention.qualityIncomplete")}</span>
      </header>
      <dl>
        <div><dt>{t("live.attention.reviewedSamples")}</dt><dd>{report.reviewedSamples} / {report.requiredSamples}</dd></div>
        <div><dt>{t("live.attention.stuckPrecision")}</dt><dd>{stuckPrecision ?? unavailable}</dd></div>
        <div>
          <dt>{t("live.attention.falsePositiveRate")}</dt>
          <dd>{falsePositiveRate === null
            ? unavailable
            : t("live.attention.rateWithSamples", { rate: falsePositiveRate, count: report.feedbackSamples })}</dd>
        </div>
        <div>
          <dt>{t("live.attention.notificationLatency")}</dt>
          <dd>{report.notificationP95Seconds === null
            ? unavailable
            : t("live.attention.seconds", { value: report.notificationP95Seconds.toFixed(2) })}</dd>
        </div>
        <div><dt>{t("live.attention.jumpSuccessRate")}</dt><dd>{jumpSuccessRate ?? unavailable}</dd></div>
        <div><dt>{t("live.attention.realAppCheck")}</dt><dd>{t(report.realAppVerified ? "live.attention.verified" : "live.attention.notVerified")}</dd></div>
      </dl>
    </aside>
  );
}

function AttentionHistory({ items }: { items: AttentionEvent[] }) {
  const { t } = useTranslation();
  if (!items.length) return <div className="attention-empty">{t("live.attention.historyEmpty")}</div>;
  return (
    <ul className="attention-history-list">
      {items.map((attention) => (
        <li key={attention.id}>
          <strong>{t(`live.attention.kind.${attention.kind}`)}</strong>
          <span>{attention.projectLabel || agentName(attention.agent)}</span>
          <small>{t(`live.attention.state.${attention.state}`)}</small>
        </li>
      ))}
    </ul>
  );
}

function liveReason(session: LiveSession, t: TFunction): string | undefined {
  const attention = session.pulse.attentionSignal.value;
  if (attention === "blocking-error") return t("live.reason.error");
  if (session.status === "paused") return t("live.reason.paused");
  if (attention !== "needs-you") return undefined;
  const tool = session.actions[session.actions.length - 1]?.label;
  return tool && tool !== "PermissionRequest"
    ? t("live.reason.waiting", { tool })
    : t("live.reason.waitingGeneric");
}

function pulseValue(dimension: WorkPulseDimension, t: TFunction): string {
  if (dimension.availability === "unknown") return t("live.pulse.unknown");
  if (dimension.availability === "not-recorded" || !dimension.value) {
    return t("live.pulse.notRecorded");
  }
  return t(`live.pulse.value.${dimension.value}`, { defaultValue: dimension.value });
}

function statusIcon(status: LiveSession["status"] | string) {
  if (status === "waiting" || status === "error") return <CircleAlert size={15} />;
  if (status === "paused") return <CirclePause size={15} />;
  if (status === "completed") return <Check size={15} />;
  return <Waves size={15} />;
}

export function LiveSessionCard({ session, locale }: { session: LiveSession; locale: Locale }) {
  const { t } = useTranslation();
  const reason = liveReason(session, t);
  const experimental = session.pulse.lifecycle.availability !== "available";
  const visibleActions = experimental ? [] : session.actions;
  const headline = experimental
    ? pulseValue(session.pulse.workPhase, t)
    : pulseValue(session.pulse.lifecycle, t);
  return (
    <article className={`live-session-card status-${experimental ? "limited" : session.status}`}>
      <header>
        <AgentBadge agent={session.agent} />
        <div>
          <strong>{session.projectLabel}</strong>
          <small>{agentName(session.agent)} · {session.origin ? t(`live.origin.${session.origin}`) : t("live.origin.unknown")}</small>
        </div>
        <span className="live-status">{statusIcon(experimental ? "limited" : session.status)}{headline}</span>
      </header>
      <div className="live-pulse-grid">
        <div><span>{t("live.pulse.lifecycle")}</span><strong>{pulseValue(session.pulse.lifecycle, t)}</strong></div>
        <div><span>{t("live.pulse.workPhase")}</span><strong>{pulseValue(session.pulse.workPhase, t)}</strong></div>
        <div><span>{t("live.pulse.attention")}</span><strong>{pulseValue(session.pulse.attentionSignal, t)}</strong></div>
        <div>
          <span>{t("live.pulse.freshness")}</span>
          <strong>{pulseValue(session.pulse.freshness, t)}</strong>
          <small><Clock3 size={12} />{formatTime(session.updatedAt, locale)}</small>
        </div>
      </div>
      {reason ? <p className="live-reason">{reason}</p> : null}
      <div className="live-actions" aria-label={t("live.recentActions")}>
        {visibleActions.map((action, index) => (
          <span key={`${action.occurredAt}-${index}`}>
            <i />
            {t(`live.action.${action.kind}`, { defaultValue: action.label })}
          </span>
        ))}
      </div>
      <footer>
        <span>{t("live.structuredOnly")}</span>
        <button className="button secondary" onClick={() => void api.jumpToLiveSession(session.id)}>
          {t("live.jump")}<ArrowUpRight size={13} />
        </button>
      </footer>
    </article>
  );
}

export function TimelineList({ points, locale }: { points: LiveTimelinePoint[]; locale: Locale }) {
  const { t } = useTranslation();
  if (!points.length) return <EmptyState title={t("live.timelineEmpty")} body={t("live.timelineEmptyBody")} />;
  return (
    <ol className="live-timeline-list">
      {points.map((point) => (
        <li key={point.id} className={`status-${point.status}`}>
          <time>{formatTime(point.occurredAt, locale)}</time>
          <AgentBadge agent={point.agent} compact />
          <div>
            <strong>{point.projectLabel || agentName(point.agent)}</strong>
            <small>{t(`live.status.${point.status}`, { defaultValue: point.status })} · {point.eventName}</small>
          </div>
        </li>
      ))}
    </ol>
  );
}

export function HistoryList({
  items,
  locale,
  onOpenSession,
}: {
  items: LiveHistoryItem[];
  locale: Locale;
  onOpenSession: (sessionId: string) => void;
}) {
  const { t } = useTranslation();
  if (!items.length) return <EmptyState title={t("live.historyEmpty")} body={t("live.historyEmptyBody")} />;
  return (
    <ul className="live-history-list">
      {items.map((item) => (
        <li key={item.id} className={`status-${item.status}`}>
          <span className="live-status">{statusIcon(item.status)}{t(`live.status.${item.status}`, { defaultValue: item.status })}</span>
          <div>
            <strong>{item.projectLabel || agentName(item.agent)}</strong>
            <small>{agentName(item.agent)} · {item.eventName}</small>
          </div>
          <time>{formatDateTime(item.occurredAt, locale)}</time>
          <button
            className="button subtle live-history-jump"
            disabled={!item.sessionId}
            title={item.sessionId ? undefined : t("live.historyJumpUnavailable")}
            onClick={() => item.sessionId && onOpenSession(item.sessionId)}
          >
            {t("live.jump")}<ArrowUpRight size={12} />
          </button>
        </li>
      ))}
    </ul>
  );
}

export function LivePage({ locale }: { locale: Locale }) {
  const { t } = useTranslation();
  const capabilityNames = sourceCapabilityNameGroups(locale === "zh-CN" ? "、" : ", ");
  const setPage = useUiStore((state) => state.setPage);
  const snapshot = useLiveSnapshot();
  const [attentionJumpErrors, setAttentionJumpErrors] = useState<ReadonlySet<string>>(new Set());
  const activity = useQuery({
    queryKey: ["live-activity"],
    queryFn: api.liveActivity,
    refetchInterval: 5_000,
  });
  const quality = useQuery({
    queryKey: ["attention-quality"],
    queryFn: api.attentionQuality,
    refetchInterval: 30_000,
  });
  if (snapshot.isLoading) return <LoadingState />;
  if (snapshot.isError || !snapshot.data) return <ErrorState retry={() => void snapshot.refetch()} />;
  const data = snapshot.data;
  const activityData = activity.data;
  const attention = activityData?.attention ?? [];
  const currentAttention = attention.filter((item) =>
    item.state === "open" || item.state === "acknowledged");
  const attentionHistory = attention.filter((item) =>
    item.state === "resolved" || item.state === "ignored" || item.state === "expired");
  const updateAttention = async (id: string, feedback: AttentionFeedback) => {
    await api.setAttentionFeedback(id, feedback);
    setAttentionJumpErrors((current) => {
      const next = new Set(current);
      next.delete(id);
      return next;
    });
    await Promise.all([activity.refetch(), snapshot.refetch()]);
  };
  const jumpToAttention = async (id: string) => {
    try {
      await api.jumpToAttention(id);
      setAttentionJumpErrors((current) => {
        const next = new Set(current);
        next.delete(id);
        return next;
      });
    } catch {
      setAttentionJumpErrors((current) => new Set(current).add(id));
    } finally {
      await Promise.all([activity.refetch(), snapshot.refetch()]);
    }
  };

  return (
    <div className="page live-page">
      <header className="live-hero">
        <div>
          <span className="eyebrow"><RadioTower size={13} />{t("live.eyebrow")}</span>
          <h1>{t("live.title")}</h1>
          <p>{t("live.description", capabilityNames)}</p>
        </div>
        <div className={`live-signal-summary state-${data.hookStatus.state}`}>
          <span className="live-orbit"><i /><i /><i /></span>
          <div><strong>{data.activeCount}</strong><span>{t("live.activeAgents")}</span></div>
          <small>{data.hookStatus.socketReady ? t("live.socketReady") : t("live.socketUnavailable")}</small>
        </div>
      </header>

      <aside className={`live-hook-strip state-${data.hookStatus.state}`} role="status">
        <ShieldCheck size={16} />
        <div>
          <strong>{t(`settings.liveState.${data.hookStatus.state}`)}</strong>
          <p>{t("live.hookStripBody")}</p>
        </div>
        <button className="button subtle" onClick={() => setPage("settings")}>
          <Settings2 size={13} />{t("live.openSettings")}<ArrowRight size={13} />
        </button>
      </aside>

      <section className="live-workspace">
        <header className="section-heading">
          <div><h2>{t("live.sessions")}</h2><p>{t("live.sessionsBody")}</p></div>
          <span className="panel-kicker">{t("live.priorityOrder")}</span>
        </header>
        {data.sessions.length ? (
          <div className="live-session-grid">
            {data.sessions.map((session) => <LiveSessionCard key={session.id} session={session} locale={locale} />)}
          </div>
        ) : (
          <div className="live-empty">
            <span className="live-empty-meter"><i /></span>
            <div><h3>{t("live.emptyTitle")}</h3><p>{t("live.emptyBody", capabilityNames)}</p></div>
          </div>
        )}
      </section>

      <section className="live-workspace attention-workspace">
        <header className="section-heading">
          <div><h2>{t("live.attention.queueTitle")}</h2><p>{t("live.attention.queueBody")}</p></div>
          <span className="panel-kicker">{t("live.priorityOrder")}</span>
        </header>
        {activity.isLoading && !activityData ? <LoadingState /> : activity.isError ? (
          <ErrorState retry={() => void activity.refetch()} />
        ) : (
          <AttentionQueue
            items={currentAttention}
            jumpErrors={attentionJumpErrors}
            onFeedback={(id, feedback) => void updateAttention(id, feedback)}
            onJump={(id) => void jumpToAttention(id)}
          />
        )}
        {quality.data ? <AttentionQualityGate report={quality.data} /> : null}
      </section>

      <div className="live-split">
        <section className="live-panel">
          <header>
            <div><Clock3 size={16} /><div><h2>{t("live.timeline")}</h2><p>{t("live.timelineBody")}</p></div></div>
          </header>
          {activity.isLoading && !activityData ? <LoadingState /> : activity.isError ? (
            <ErrorState retry={() => void activity.refetch()} />
          ) : (
            <TimelineList points={activityData?.timeline ?? []} locale={locale} />
          )}
        </section>
        <section className="live-panel">
          <header>
            <div><CircleAlert size={16} /><div><h2>{t("live.attention.historyTitle")}</h2><p>{t("live.attention.historyBody")}</p></div></div>
          </header>
          {activity.isLoading && !activityData ? <LoadingState /> : activity.isError ? (
            <ErrorState retry={() => void activity.refetch()} />
          ) : (
            <AttentionHistory items={attentionHistory} />
          )}
        </section>
      </div>

      <footer className="live-privacy-note">
        <ShieldCheck size={15} />
        <p><strong>{t("live.privacyTitle")}</strong>{t("live.privacyBody")}</p>
      </footer>
    </div>
  );
}
