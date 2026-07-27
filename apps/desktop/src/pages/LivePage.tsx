import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { TFunction } from "i18next";
import {
  ArrowUpRight,
  Check,
  CircleAlert,
  Clock3,
  PanelTop,
  RadioTower,
  RefreshCw,
  ShieldCheck,
  Unplug,
  Waves,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { AgentBadge, ErrorState, LoadingState, Toggle } from "../components/ui";
import { api } from "../lib/api";
import { agentName, formatTime } from "../lib/format";
import { useLiveSnapshot } from "../lib/useLiveSnapshot";
import type { LiveSession, Locale } from "../types";

function liveReason(session: LiveSession, t: TFunction): string | undefined {
  if (session.status === "error") return t("live.reason.error");
  if (session.status !== "waiting") return undefined;
  const tool = session.actions[session.actions.length - 1]?.label;
  return tool && tool !== "PermissionRequest"
    ? t("live.reason.waiting", { tool })
    : t("live.reason.waitingGeneric");
}

function statusIcon(status: LiveSession["status"]) {
  if (status === "waiting" || status === "error") return <CircleAlert size={15} />;
  if (status === "completed") return <Check size={15} />;
  return <Waves size={15} />;
}

function LiveSessionCard({ session, locale }: { session: LiveSession; locale: Locale }) {
  const { t } = useTranslation();
  const reason = liveReason(session, t);
  return (
    <article className={`live-session-card status-${session.status}`}>
      <header>
        <AgentBadge agent={session.agent} />
        <div>
          <strong>{session.projectLabel}</strong>
          <small>{agentName(session.agent)} · {session.origin ? t(`live.origin.${session.origin}`) : t("live.origin.unknown")}</small>
        </div>
        <span className="live-status">{statusIcon(session.status)}{t(`live.status.${session.status}`)}</span>
      </header>
      <div className="live-phase">
        <span>{t("live.currentPhase")}</span>
        <strong>{t(`live.phase.${session.phase}`, { defaultValue: session.phase })}</strong>
        <small><Clock3 size={12} />{formatTime(session.updatedAt, locale)}</small>
      </div>
      {reason ? <p className="live-reason">{reason}</p> : null}
      <div className="live-actions" aria-label={t("live.recentActions")}>
        {session.actions.map((action, index) => (
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

export function LivePage({ locale }: { locale: Locale }) {
  const { t } = useTranslation();
  const client = useQueryClient();
  const snapshot = useLiveSnapshot();
  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  const setNotch = useMutation({
    mutationFn: (enabled: boolean) => api.setSetting("notchEnabled", String(enabled)),
    onSuccess: async () => {
      await Promise.all([
        client.invalidateQueries({ queryKey: ["settings"] }),
        client.invalidateQueries({ queryKey: ["menu-snapshot"] }),
      ]);
    },
  });
  const repair = useMutation({
    mutationFn: api.repairLiveHooks,
    onSuccess: async () => {
      await api.setSetting("liveHooksEnabled", "true");
      await client.invalidateQueries({ queryKey: ["live-snapshot"] });
      await client.invalidateQueries({ queryKey: ["settings"] });
    },
  });
  if (snapshot.isLoading) return <LoadingState />;
  if (snapshot.isError || !snapshot.data) return <ErrorState retry={() => void snapshot.refetch()} />;
  const data = snapshot.data;
  return (
    <div className="page live-page">
      <header className="live-hero">
        <div>
          <span className="eyebrow"><RadioTower size={13} />{t("live.eyebrow")}</span>
          <h1>{t("live.title")}</h1>
          <p>{t("live.description")}</p>
        </div>
        <div className="live-hero-side">
          <div className={`live-signal-summary state-${data.hookStatus.state}`}>
            <span className="live-orbit"><i /><i /><i /></span>
            <div><strong>{data.activeCount}</strong><span>{t("live.activeAgents")}</span></div>
            <small>{data.hookStatus.socketReady ? t("live.socketReady") : t("live.socketUnavailable")}</small>
          </div>
          <div className="live-notch-control">
            <span><PanelTop size={15} /><span><strong>{t("live.notchControl")}</strong><small>{t("live.notchControlBody")}</small></span></span>
            <Toggle
              checked={settings.data?.notchEnabled === "true"}
              disabled={settings.isLoading || setNotch.isPending}
              onCheckedChange={(value) => setNotch.mutate(value)}
              label={t("live.notchControl")}
            />
          </div>
        </div>
      </header>

      <section className="live-integrations">
        <header>
          <div><ShieldCheck size={17} /><span><strong>{t("live.integrations")}</strong><small>{t("live.integrationsBody")}</small></span></div>
          <button className="button subtle" disabled={repair.isPending} onClick={() => repair.mutate()}>
            <RefreshCw size={13} />{repair.isPending ? t("actions.refreshing") : t("live.repair")}
          </button>
        </header>
        <div>
          {data.hookStatus.providers.map((provider) => (
            <article key={provider.provider} className={provider.installed ? "ready" : provider.available ? "attention" : "missing"}>
              <AgentBadge agent={provider.provider} compact />
              <span><strong>{agentName(provider.provider)}</strong><small>{t(`live.hook.${provider.detail}`)}</small></span>
              {provider.installed ? <Check size={14} /> : <Unplug size={14} />}
            </article>
          ))}
        </div>
      </section>

      <section className="live-workspace">
        <header className="section-heading">
          <div><span className="section-index">LIVE</span><h2>{t("live.sessions")}</h2><p>{t("live.sessionsBody")}</p></div>
          <span className="panel-kicker">{t("live.priorityOrder")}</span>
        </header>
        {data.sessions.length ? (
          <div className="live-session-grid">
            {data.sessions.map((session) => <LiveSessionCard key={session.id} session={session} locale={locale} />)}
          </div>
        ) : (
          <div className="live-empty">
            <span className="live-empty-meter"><i /></span>
            <div><h3>{t("live.emptyTitle")}</h3><p>{t("live.emptyBody")}</p></div>
          </div>
        )}
      </section>

      <footer className="live-privacy-note">
        <ShieldCheck size={15} />
        <p><strong>{t("live.privacyTitle")}</strong>{t("live.privacyBody")}</p>
      </footer>
    </div>
  );
}
