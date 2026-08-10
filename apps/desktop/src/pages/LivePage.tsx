import { useQuery } from "@tanstack/react-query";
import type { TFunction } from "i18next";
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
  LiveHistoryItem,
  LiveSession,
  LiveTimelinePoint,
  Locale,
  WorkPulseDimension,
} from "../types";

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
  const openSessions = useUiStore((state) => state.openSessions);
  const snapshot = useLiveSnapshot();
  const activity = useQuery({
    queryKey: ["live-activity"],
    queryFn: api.liveActivity,
    refetchInterval: 5_000,
  });
  if (snapshot.isLoading) return <LoadingState />;
  if (snapshot.isError || !snapshot.data) return <ErrorState retry={() => void snapshot.refetch()} />;
  const data = snapshot.data;
  const activityData = activity.data;

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
            <div><CircleAlert size={16} /><div><h2>{t("live.history")}</h2><p>{t("live.historyBody")}</p></div></div>
          </header>
          {activity.isLoading && !activityData ? <LoadingState /> : activity.isError ? (
            <ErrorState retry={() => void activity.refetch()} />
          ) : (
            <HistoryList items={activityData?.history ?? []} locale={locale} onOpenSession={openSessions} />
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
