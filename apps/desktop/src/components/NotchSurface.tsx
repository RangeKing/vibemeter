import { listen } from "@tauri-apps/api/event";
import {
  ArrowUpRight,
  BookOpenText,
  BrainCircuit,
  Check,
  ChevronRight,
  CircleAlert,
  CircleDot,
  FilePenLine,
  Gauge,
  ListTree,
  Pin,
  ShieldCheck,
  Shrink,
  Terminal,
  X,
} from "lucide-react";
import type { CSSProperties } from "react";
import type { TFunction } from "i18next";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import claudeCodeIconUrl from "../assets/providers/claudecode.svg";
import codexIconUrl from "../assets/providers/codex.svg";
import appIconUrl from "../../src-tauri/icons/vibemeter-icon-source.png";
import { api } from "../lib/api";
import { agentName } from "../lib/format";
import { useLiveSnapshot } from "../lib/useLiveSnapshot";
import type { LiveSession, Locale, NotchUiState } from "../types";

const COMPLETION_CUE_MS = 5_000;
const ACTIVE_STATUSES = new Set<LiveSession["status"]>(["waiting", "error", "running"]);
const MULTI_PROVIDER_LEFT_WING_WIDTH = 88;
const MIN_SINGLE_PROJECT_LEFT_WING_WIDTH = 88;
const MAX_SINGLE_PROJECT_LEFT_WING_WIDTH = 154;
const EXPANDED_NOTCH_WIDTH = 440;
const MIN_EXPANDED_HEIGHT = 150;
const MAX_EXPANDED_HEIGHT = 352;
const DEFAULT_NOTCH_STATE: NotchUiState = {
  available: true,
  enabled: true,
  expanded: false,
  pinned: false,
  hasActivity: false,
  hardwareWidth: 180,
  hardwareHeight: 34,
  leftWingWidth: 88,
  rightWingWidth: 98,
  expandedHeight: 168,
};

export function formatLiveElapsed(startedAt: string, endedAt: number): string {
  const startedAtMs = new Date(startedAt).getTime();
  if (!Number.isFinite(startedAtMs) || !Number.isFinite(endedAt)) return "—";
  const elapsedSeconds = Math.max(0, Math.floor((endedAt - startedAtMs) / 1_000));
  const hours = Math.floor(elapsedSeconds / 3_600);
  const minutes = Math.floor((elapsedSeconds % 3_600) / 60);
  const seconds = elapsedSeconds % 60;
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`
    : `${minutes}:${String(seconds).padStart(2, "0")}`;
}

function liveReason(session: LiveSession, t: TFunction): string | undefined {
  if (session.status === "error") return t("live.reason.error");
  if (session.status !== "waiting") return undefined;
  const tool = session.actions[session.actions.length - 1]?.label;
  return tool && tool !== "PermissionRequest"
    ? t("live.reason.waiting", { tool })
    : t("live.reason.waitingGeneric");
}

function ProviderMark({
  agent,
  size = 14,
}: {
  agent: LiveSession["agent"];
  size?: number;
}) {
  const iconUrl = agent === "claude-code" ? claudeCodeIconUrl : codexIconUrl;
  return (
    <span
      className={`notch-provider-mark provider-mark-${agent}`}
      style={{
        width: size,
        height: size,
        "--notch-provider-icon": `url("${iconUrl}")`,
      } as CSSProperties}
      aria-hidden="true"
    />
  );
}

function ProviderCount({
  agent,
  count,
}: {
  agent: LiveSession["agent"];
  count: number;
}) {
  if (!count) return null;
  return (
    <span className={`notch-provider-count provider-${agent}`}>
      <ProviderMark agent={agent} />
      <b>×{count}</b>
    </span>
  );
}

function liveActionKey(action: LiveSession["actions"][number]): string {
  return `${action.occurredAt}:${action.kind}:${action.label}`;
}

function NotchActionFlow({
  actions,
  t,
}: {
  actions: LiveSession["actions"];
  t: TFunction;
}) {
  const flowRef = useRef<HTMLDivElement>(null);
  const previousPositions = useRef(new Map<string, DOMRect>());
  const signature = actions.map(liveActionKey).join("|");

  useLayoutEffect(() => {
    const flow = flowRef.current;
    if (!flow) return;
    const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const nextPositions = new Map<string, DOMRect>();
    for (const element of flow.querySelectorAll<HTMLElement>("[data-action-key]")) {
      const key = element.dataset.actionKey;
      if (!key) continue;
      const currentPosition = element.getBoundingClientRect();
      const previousPosition = previousPositions.current.get(key);
      nextPositions.set(key, currentPosition);
      if (reduceMotion || typeof element.animate !== "function") continue;
      element.getAnimations().forEach((animation) => animation.cancel());
      if (previousPosition) {
        const offset = previousPosition.left - currentPosition.left;
        if (Math.abs(offset) > 0.5) {
          element.animate(
            [
              { transform: `translateX(${offset}px)` },
              { transform: "translateX(0)" },
            ],
            { duration: 320, easing: "cubic-bezier(.16, 1, .3, 1)" },
          );
        }
      } else {
        element.animate(
          [
            { opacity: 0, transform: "translateX(8px) scale(.96)" },
            { opacity: 1, transform: "translateX(0) scale(1)" },
          ],
          { duration: 280, easing: "cubic-bezier(.16, 1, .3, 1)" },
        );
      }
    }
    previousPositions.current = nextPositions;
  }, [signature]);

  return (
    <div className="notch-action-flow" ref={flowRef}>
      {actions.map((action, index) => {
        const key = liveActionKey(action);
        return (
          <span className="notch-action-group" data-action-key={key} key={key}>
            <span className="notch-action-step">
              {t(`live.action.${action.kind}`, { defaultValue: action.label })}
            </span>
            {index < actions.length - 1 ? (
              <ChevronRight className="notch-action-arrow" size={8} strokeWidth={2.4} aria-hidden="true" />
            ) : null}
          </span>
        );
      })}
    </div>
  );
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
  else if (phase === "planning") Icon = ListTree;
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

export function pickRightWingSession(sessions: LiveSession[], now: number) {
  const waiting = sessions.find((session) => session.status === "waiting");
  if (waiting) return waiting;
  const error = sessions.find((session) => session.status === "error");
  if (error) return error;
  const completion = sessions.find(
    (session) =>
      session.status === "completed" &&
      now - new Date(session.updatedAt).getTime() < COMPLETION_CUE_MS,
  );
  if (completion) return completion;
  return sessions.find((session) => session.status === "running");
}

export function activeProviderCounts(sessions: LiveSession[]) {
  const active = sessions.filter((session) => ACTIVE_STATUSES.has(session.status));
  return {
    active,
    codex: active.filter((session) => session.agent === "codex").length,
    claudeCode: active.filter((session) => session.agent === "claude-code").length,
  };
}

function estimatedProjectLabelWidth(label: string) {
  return Array.from(label.trim()).reduce((width, character) => {
    if (/[\u2E80-\u9FFF\uF900-\uFAFF]/u.test(character)) return width + 9;
    if (/[MW@#%]/.test(character)) return width + 7;
    if (/[A-Z0-9]/.test(character)) return width + 5.8;
    return width + 5;
  }, 0);
}

export function leftWingWidthForSession(session?: Pick<LiveSession, "projectLabel">) {
  if (!session) return MULTI_PROVIDER_LEFT_WING_WIDTH;
  const contentWidth = 16 + 6 + estimatedProjectLabelWidth(session.projectLabel) + 16;
  return Math.round(
    Math.min(
      MAX_SINGLE_PROJECT_LEFT_WING_WIDTH,
      Math.max(MIN_SINGLE_PROJECT_LEFT_WING_WIDTH, contentWidth),
    ),
  );
}

export function expandedHeightForSessions(
  sessions: Pick<LiveSession, "status">[],
  hardwareHeight: number,
) {
  if (!sessions.length) return MIN_EXPANDED_HEIGHT;
  const cardHeights = sessions.reduce(
    (height, session) =>
      height + 80 + (session.status === "waiting" || session.status === "error" ? 18 : 0),
    0,
  );
  const gaps = Math.max(0, sessions.length - 1) * 7;
  const desiredHeight = hardwareHeight + 36 + 20 + cardHeights + gaps;
  return Math.round(Math.min(MAX_EXPANDED_HEIGHT, Math.max(MIN_EXPANDED_HEIGHT, desiredHeight)));
}

export function NotchSurface({ locale }: { locale: Locale }) {
  const { t } = useTranslation();
  const snapshot = useLiveSnapshot();
  const [notchState, setNotchState] = useState(DEFAULT_NOTCH_STATE);
  const [keepExpandedDuringClose, setKeepExpandedDuringClose] = useState(false);
  const [now, setNow] = useState(Date.now());
  const lastActivity = useRef<boolean | undefined>(undefined);
  const sessions = snapshot.data?.sessions ?? [];
  const providerCounts = useMemo(() => activeProviderCounts(sessions), [sessions]);
  const activeSessions = providerCounts.active;
  const rightSession = pickRightWingSession(sessions, now);
  const recentCompletion =
    rightSession?.status === "completed" &&
    now - new Date(rightSession.updatedAt).getTime() < COMPLETION_CUE_MS;
  const hasActivity = activeSessions.length > 0 || recentCompletion;
  const singleWingSession =
    activeSessions.length === 1
      ? activeSessions[0]
      : activeSessions.length === 0 && recentCompletion
        ? rightSession
        : undefined;
  const visibleSessions =
    activeSessions.length > 0 ? activeSessions : recentCompletion && rightSession ? [rightSession] : [];
  const codexCount = providerCounts.codex;
  const claudeCount = providerCounts.claudeCode;
  const desiredLeftWingWidth = leftWingWidthForSession(singleWingSession);
  const desiredExpandedHeight = expandedHeightForSessions(
    visibleSessions,
    notchState.hardwareHeight,
  );
  const style = {
    "--notch-hardware-width": `${notchState.hardwareWidth}px`,
    "--notch-hardware-height": `${notchState.hardwareHeight}px`,
    "--notch-left-wing-width": `${notchState.leftWingWidth}px`,
    "--notch-right-wing-width": `${notchState.rightWingWidth}px`,
    "--notch-collapsed-side-inset": `${Math.max(
      0,
      (EXPANDED_NOTCH_WIDTH - notchState.hardwareWidth) / 2,
    )}px`,
  } as CSSProperties;

  const hasTimedSession = sessions.some(
    (session) => ACTIVE_STATUSES.has(session.status) || session.status === "completed",
  );

  useEffect(() => {
    if (!hasTimedSession) return;
    setNow(Date.now());
    const interval = window.setInterval(() => setNow(Date.now()), 1_000);
    return () => window.clearInterval(interval);
  }, [hasTimedSession]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void api.notchState().then((state) => {
      if (!disposed) setNotchState(state);
    });
    void listen<NotchUiState>("notch-state", (event) => {
      if (!disposed) setNotchState(event.payload);
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (notchState.expanded) {
      setKeepExpandedDuringClose(true);
      return;
    }
    if (!keepExpandedDuringClose) return;
    const timeout = window.setTimeout(() => setKeepExpandedDuringClose(false), 260);
    return () => window.clearTimeout(timeout);
  }, [keepExpandedDuringClose, notchState.expanded]);

  useEffect(() => {
    if (lastActivity.current === hasActivity) return;
    lastActivity.current = hasActivity;
    void api.setNotchActivity(hasActivity);
  }, [hasActivity]);

  useEffect(() => {
    void api.setNotchLayout(desiredLeftWingWidth, desiredExpandedHeight);
  }, [desiredExpandedHeight, desiredLeftWingWidth]);

  const toggleExpanded = async (expanded: boolean) => {
    await api.setNotchExpanded(expanded);
  };
  const togglePinned = async () => {
    await api.setNotchPinned(!notchState.pinned);
  };

  if (!notchState.enabled) return null;

  const showExpandedSurface = notchState.expanded || keepExpandedDuringClose;
  const isClosing = showExpandedSurface && !notchState.expanded;

  if (!showExpandedSurface) {
    return (
      <div
        className={`notch-island notch-island-collapsed${hasActivity ? " has-activity" : " is-idle"}`}
        style={style}
      >
        <button
          className="notch-collapsed-shell"
          onClick={() => void toggleExpanded(true)}
          aria-label={t("notch.open")}
        >
          <span className={`notch-wing notch-wing-left${singleWingSession ? "" : " is-multi"}`}>
            {singleWingSession ? (
              <span className={`notch-single-project provider-${singleWingSession.agent}`}>
                <ProviderMark agent={singleWingSession.agent} />
                <strong>{singleWingSession.projectLabel}</strong>
              </span>
            ) : (
              <span className="notch-provider-cluster">
                <ProviderCount agent="codex" count={codexCount} />
                <ProviderCount agent="claude-code" count={claudeCount} />
              </span>
            )}
          </span>
          <span className="notch-hardware" />
          <span className={`notch-wing notch-wing-right status-${rightSession?.status ?? "idle"}`}>
            {rightSession ? (
              <>
                <AgentActivityGlyph session={rightSession} compact />
                <strong>{t(`live.phase.${rightSession.phase}`, { defaultValue: rightSession.phase })}</strong>
              </>
            ) : null}
          </span>
        </button>
      </div>
    );
  }

  return (
    <section
      className={`notch-island notch-island-expanded${isClosing ? " is-closing" : ""}`}
      style={style}
    >
      <header className="notch-expanded-bridge">
        <span className="notch-expanded-left">
          <ProviderCount agent="codex" count={codexCount} />
          <ProviderCount agent="claude-code" count={claudeCount} />
        </span>
        <span className="notch-hardware" aria-hidden="true" />
        <span className="notch-expanded-controls">
          <button
            className={notchState.pinned ? "is-pinned" : ""}
            onClick={() => void togglePinned()}
            aria-label={notchState.pinned ? t("notch.unpin") : t("notch.pin")}
            aria-pressed={notchState.pinned}
          >
            <Pin size={13} />
          </button>
          <button onClick={() => void toggleExpanded(false)} aria-label={t("actions.close")}>
            <X size={14} />
          </button>
        </span>
      </header>
      <div className="notch-expanded-heading">
        <span>
          <img className="notch-brand-icon" src={appIconUrl} alt="" />
          <strong>VibeMeter</strong>
        </span>
        <small>{t("notch.activeCount", { count: activeSessions.length })}</small>
      </div>
      <div className={`notch-session-list ${visibleSessions.length ? "" : "is-empty"}`}>
        {visibleSessions.length ? (
          visibleSessions.map((session, index) => {
            const reason = liveReason(session, t);
            return (
              <article
                key={session.id}
                className={`status-${session.status}`}
                style={{ "--notch-session-index": index } as CSSProperties}
              >
                <div className="notch-session-top">
                  <span className={`notch-agent-dot provider-${session.agent}`}>
                    <ProviderMark agent={session.agent} size={14} />
                  </span>
                  <span>
                    <strong>{session.projectLabel}</strong>
                    <small>
                      {agentName(session.agent)} · {t("notch.elapsed", {
                        value: formatLiveElapsed(
                          session.startedAt,
                          session.status === "completed"
                            ? new Date(session.updatedAt).getTime()
                            : now,
                        ),
                      })}
                    </small>
                  </span>
                  <span className="notch-phase">
                    <AgentActivityGlyph session={session} compact />
                    {t(`live.phase.${session.phase}`, { defaultValue: session.phase })}
                  </span>
                </div>
                {reason ? <p>{reason}</p> : null}
                <footer>
                  <NotchActionFlow actions={session.actions} t={t} />
                  <button
                    onClick={() => void api.jumpToLiveSession(session.id)}
                    aria-label={t("live.jump")}
                  >
                    <ArrowUpRight size={13} />
                  </button>
                </footer>
              </article>
            );
          })
        ) : (
          <div className="notch-empty">
            <strong>VibeMeter</strong>
            <span>{t("notch.noActivity")}</span>
          </div>
        )}
      </div>
    </section>
  );
}
