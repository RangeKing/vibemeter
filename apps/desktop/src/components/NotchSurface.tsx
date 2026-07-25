import {
  ArrowUpRight,
  BookOpenText,
  BrainCircuit,
  Check,
  CircleAlert,
  CircleDot,
  FilePenLine,
  Gauge,
  ShieldCheck,
  Shrink,
  Terminal,
  X,
} from "lucide-react";
import type { TFunction } from "i18next";
import { useState } from "react";
import { useTranslation } from "react-i18next";
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

export function AgentActivityGlyph({
  session,
  compact = false,
}: {
  session?: Pick<LiveSession, "phase" | "status">;
  compact?: boolean;
}) {
  const status = session?.status ?? "idle";
  const phase = session?.phase ?? "ready";
  let Icon = Gauge;
  if (status === "waiting" || status === "error") Icon = CircleAlert;
  else if (status === "completed") Icon = Check;
  else if (phase === "thinking") Icon = BrainCircuit;
  else if (phase === "reading") Icon = BookOpenText;
  else if (phase === "editing") Icon = FilePenLine;
  else if (phase === "verifying") Icon = ShieldCheck;
  else if (phase === "running-tool") Icon = Terminal;
  else if (phase === "compacting") Icon = Shrink;
  else if (phase === "ready") Icon = CircleDot;
  return (
    <span
      className={`notch-activity-glyph status-${status} phase-${phase}${compact ? " compact" : ""}`}
      aria-hidden="true"
    >
      <Icon size={compact ? 11 : 13} strokeWidth={2.15} />
    </span>
  );
}

export function NotchSurface({ locale }: { locale: Locale }) {
  const { t } = useTranslation();
  const snapshot = useLiveSnapshot();
  const [expanded, setExpanded] = useState(false);
  const sessions = snapshot.data?.sessions ?? [];
  const urgent = sessions[0];
  const toggle = async (value: boolean) => {
    setExpanded(value);
    await api.setNotchExpanded(value);
  };

  if (!expanded) {
    return (
      <button className={`notch-capsule ${urgent ? `status-${urgent.status}` : "status-idle"}`} onClick={() => void toggle(true)}>
        <AgentActivityGlyph session={urgent} />
        {urgent ? (
          <>
            <span className="notch-agent">{agentName(urgent.agent)}</span>
            <strong className="notch-project">{urgent.projectLabel}</strong>
            <small className="notch-phase-label">{t(`live.phase.${urgent.phase}`, { defaultValue: urgent.phase })}</small>
          </>
        ) : (
          <>
            <strong>{t("notch.ready")}</strong>
            <small>{t("notch.waiting")}</small>
          </>
        )}
        {sessions.length > 1 ? <span className="notch-count">+{sessions.length - 1}</span> : null}
      </button>
    );
  }

  return (
    <section className="notch-expanded">
      <header>
        <span><AgentActivityGlyph session={urgent} compact /><strong>VibeMeter</strong><small>{t("notch.activeCount", { count: snapshot.data?.activeCount ?? 0 })}</small></span>
        <button onClick={() => void toggle(false)} aria-label={t("actions.close")}><X size={14} /></button>
      </header>
      <div className="notch-session-list">
        {sessions.length ? sessions.map((session) => {
          const reason = liveReason(session, t);
          return <article key={session.id} className={`status-${session.status}`}>
            <div className="notch-session-top">
              <span className="notch-agent-dot">{session.agent === "codex" ? "C" : "A"}</span>
              <span><strong>{session.projectLabel}</strong><small>{agentName(session.agent)} · {formatTime(session.updatedAt, locale)}</small></span>
              <span className="notch-phase"><AgentActivityGlyph session={session} compact />{t(`live.phase.${session.phase}`, { defaultValue: session.phase })}</span>
            </div>
            {reason ? <p>{reason}</p> : null}
            <footer>
              <div>{session.actions.map((action, index) => <span key={`${action.occurredAt}-${index}`}>{t(`live.action.${action.kind}`, { defaultValue: action.label })}</span>)}</div>
              <button onClick={() => void api.jumpToLiveSession(session.id)} aria-label={t("live.jump")}><ArrowUpRight size={13} /></button>
            </footer>
          </article>;
        }) : (
          <div className="notch-empty"><AgentActivityGlyph compact /><strong>{t("notch.ready")}</strong><span>{t("notch.waiting")}</span></div>
        )}
      </div>
    </section>
  );
}
