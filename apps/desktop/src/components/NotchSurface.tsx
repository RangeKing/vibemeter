import { listen } from "@tauri-apps/api/event";
import {
  ArrowUpRight,
  BookOpenText,
  BrainCircuit,
  Check,
  ChevronDown,
  ChevronRight,
  CircleAlert,
  CirclePause,
  CircleDot,
  FilePenLine,
  Gauge,
  ListTree,
  Pin,
  ShieldCheck,
  Shrink,
  Terminal,
  Trash2,
  Undo2,
  X,
} from "lucide-react";
import type { CSSProperties, MouseEvent as ReactMouseEvent, PointerEvent as ReactPointerEvent } from "react";
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
const NOTCH_CLOSE_TRANSITION_MS = 300;
const MIN_EXPANDED_HEIGHT = 150;
const EXPANDED_HEIGHT_SLOP = 2;
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

export function liveElapsedEnd(session: LiveSession, now: number): number {
  if (session.status === "running") return now;
  const endedAt = new Date(session.activityEndedAt ?? session.updatedAt).getTime();
  return Number.isFinite(endedAt) ? endedAt : now;
}

function liveReason(session: LiveSession, t: TFunction): string | undefined {
  if (session.status === "error") return t("live.reason.error");
  if (session.status === "paused") return t("live.reason.paused");
  if (session.status !== "waiting") return undefined;
  const tool = session.actions[session.actions.length - 1]?.label;
  return tool && tool !== "PermissionRequest"
    ? t("live.reason.waiting", { tool })
    : t("live.reason.waitingGeneric");
}

function notchPhaseLabel(phase: LiveSession["phase"], t: TFunction): string {
  return t(`notch.phase.${phase}`, {
    defaultValue: t(`live.phase.${phase}`, { defaultValue: phase }),
  });
}

function notchActionLabel(action: LiveSession["actions"][number], t: TFunction): string {
  return t(`notch.action.${action.kind}`, {
    defaultValue: t(`live.action.${action.kind}`, { defaultValue: action.label }),
  });
}

function ProviderMark({
  agent,
  size = 14,
}: {
  agent: LiveSession["agent"];
  size?: number;
}) {
  const iconUrl = agent === "claude-code"
    ? claudeCodeIconUrl
    : agent === "codex"
      ? codexIconUrl
      : undefined;
  return (
    <span
      className={`notch-provider-mark provider-mark-${agent}${iconUrl ? "" : " is-letter"}`}
      style={iconUrl ? {
        width: size,
        height: size,
        "--notch-provider-icon": `url("${iconUrl}")`,
      } as CSSProperties : { width: size, height: size }}
      aria-hidden="true"
    >{iconUrl ? null : <span>{agent === "kimi-code" ? "K" : "Z"}</span>}</span>
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
              {notchActionLabel(action, t)}
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
  else if (status === "paused") Icon = CirclePause;
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
    kimiCode: active.filter((session) => session.agent === "kimi-code").length,
    zcode: active.filter((session) => session.agent === "zcode").length,
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

export function collapsedMorphInsets(
  hardwareWidth: number,
  leftWingWidth: number,
  rightWingWidth: number,
  hasActivity: boolean,
) {
  const hardwareInset = Math.max(0, (EXPANDED_NOTCH_WIDTH - hardwareWidth) / 2);
  return {
    left: Math.max(0, hardwareInset - (hasActivity ? leftWingWidth : 0)),
    right: Math.max(0, hardwareInset - (hasActivity ? rightWingWidth : 0)),
  };
}

export function conversationTitleForSession(
  session: Pick<LiveSession, "projectLabel" | "conversationTitle">,
) {
  const title = session.conversationTitle?.trim();
  if (!title || title.toLocaleLowerCase() === session.projectLabel.trim().toLocaleLowerCase()) {
    return undefined;
  }
  return title;
}

export function expandedHeightForSessions(
  sessions: Pick<LiveSession, "status">[],
  hardwareHeight: number,
  options: {
    completedCount?: number;
    completedExpanded?: boolean;
    completedErrorCount?: number;
    activeErrorCount?: number;
    showClearUndo?: boolean;
  } = {},
) {
  const blockHeights = sessions.map(
    (session) => 80 + (session.status === "waiting" || session.status === "error" ? 18 : 0),
  );
  const completedCount = options.completedCount ?? 0;
  if (completedCount > 0) {
    const completedCardsHeight = options.completedExpanded
      ? 7 +
        completedCount * 80 +
        (options.completedErrorCount ?? 0) * 18 +
        Math.max(0, completedCount - 1) * 7
      : 0;
    blockHeights.push(28 + completedCardsHeight);
  }
  if (options.showClearUndo) blockHeights.push(32);
  if (!blockHeights.length) return MIN_EXPANDED_HEIGHT;
  const gaps = Math.max(0, blockHeights.length - 1) * 7;
  const desiredHeight =
    hardwareHeight +
    36 +
    20 +
    blockHeights.reduce((sum, height) => sum + height, 0) +
    (options.activeErrorCount ?? 0) * 18 +
    gaps;
  return Math.round(Math.max(MIN_EXPANDED_HEIGHT, desiredHeight + EXPANDED_HEIGHT_SLOP));
}

export function NotchSurface({ locale }: { locale: Locale }) {
  const { t } = useTranslation();
  const snapshot = useLiveSnapshot();
  const [notchState, setNotchState] = useState(DEFAULT_NOTCH_STATE);
  const [notchStateReady, setNotchStateReady] = useState(false);
  const [keepExpandedDuringClose, setKeepExpandedDuringClose] = useState(false);
  const [now, setNow] = useState(Date.now());
  const [completedExpanded, setCompletedExpanded] = useState(true);
  const [sessionListScrollable, setSessionListScrollable] = useState(false);
  const [activeJumpError, setActiveJumpError] = useState<string>();
  const [completedJumpError, setCompletedJumpError] = useState<string>();
  const [clearUndo, setClearUndo] = useState<{ token: string; count: number }>();
  const lastActivity = useRef<boolean | undefined>(undefined);
  const clearUndoTimeout = useRef<number | undefined>(undefined);
  const sessionListRef = useRef<HTMLDivElement>(null);
  const pendingActions = useRef(new Set<string>());
  const sessions = snapshot.data?.sessions ?? [];
  const providerCounts = useMemo(() => activeProviderCounts(sessions), [sessions]);
  const activeSessions = providerCounts.active;
  const activeSessionSignature = activeSessions
    .map((session) => session.id)
    .sort()
    .join("|");
  const activeSessionIds = useMemo(
    () => new Set(activeSessions.map((session) => session.id)),
    [activeSessions],
  );
  const completedSessions = (snapshot.data?.completedSessions ?? []).filter(
    (completed) => !activeSessionIds.has(completed.session.id),
  );
  useEffect(() => {
    if (activeJumpError && !activeSessionIds.has(activeJumpError)) {
      setActiveJumpError(undefined);
    }
  }, [activeJumpError, activeSessionIds]);
  const rightSession = pickRightWingSession(sessions, now);
  const hasActivity = activeSessions.length > 0;
  const singleWingSession = activeSessions.length === 1 ? activeSessions[0] : undefined;
  const visibleSessions = activeSessions;
  const codexCount = providerCounts.codex;
  const claudeCount = providerCounts.claudeCode;
  const kimiCodeCount = providerCounts.kimiCode;
  const zcodeCount = providerCounts.zcode;
  const desiredLeftWingWidth = leftWingWidthForSession(singleWingSession);
  const desiredExpandedHeight = expandedHeightForSessions(
    visibleSessions,
    notchState.hardwareHeight,
    {
      completedCount: completedSessions.length,
      completedExpanded,
      completedErrorCount: completedJumpError ? 1 : 0,
      activeErrorCount: activeSessions.some(
        (session) =>
          session.id === activeJumpError && session.status !== "waiting" && session.status !== "error",
      )
        ? 1
        : 0,
      showClearUndo: Boolean(clearUndo),
    },
  );
  const collapsedInsets = collapsedMorphInsets(
    notchState.hardwareWidth,
    notchState.leftWingWidth,
    notchState.rightWingWidth,
    hasActivity,
  );
  const style = {
    "--notch-hardware-width": `${notchState.hardwareWidth}px`,
    "--notch-hardware-height": `${notchState.hardwareHeight}px`,
    "--notch-left-wing-width": `${notchState.leftWingWidth}px`,
    "--notch-right-wing-width": `${notchState.rightWingWidth}px`,
    "--notch-collapsed-left-inset": `${collapsedInsets.left}px`,
    "--notch-collapsed-right-inset": `${collapsedInsets.right}px`,
  } as CSSProperties;

  const hasTimedSession = sessions.some((session) => session.status === "running");

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
      if (!disposed) {
        setNotchState(state);
        setNotchStateReady(true);
      }
    });
    void listen<NotchUiState>("notch-state", (event) => {
      if (!disposed) {
        setNotchState(event.payload);
        setNotchStateReady(true);
      }
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
    if (!notchStateReady || !notchState.enabled || !activeSessionSignature) return;
    const ids = activeSessionSignature.split("|");
    void api.markNotchSessionsSeen(ids).then(() => snapshot.refetch());
    const interval = window.setInterval(() => {
      void api.markNotchSessionsSeen(ids);
    }, 60_000);
    return () => window.clearInterval(interval);
  }, [activeSessionSignature, notchState.enabled, notchStateReady, snapshot.refetch]);

  useEffect(
    () => () => {
      if (clearUndoTimeout.current !== undefined) {
        window.clearTimeout(clearUndoTimeout.current);
      }
    },
    [],
  );

  useEffect(() => {
    if (notchState.expanded) {
      setKeepExpandedDuringClose(true);
      return;
    }
    if (!keepExpandedDuringClose) return;
    const timeout = window.setTimeout(
      () => setKeepExpandedDuringClose(false),
      NOTCH_CLOSE_TRANSITION_MS,
    );
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

  useLayoutEffect(() => {
    const element = sessionListRef.current;
    if (!element || !notchState.expanded) {
      setSessionListScrollable(false);
      return;
    }
    const update = () => {
      setSessionListScrollable(element.scrollHeight - element.clientHeight > 2);
    };
    update();
    const observer = new ResizeObserver(update);
    observer.observe(element);
    return () => observer.disconnect();
  }, [
    activeSessionSignature,
    clearUndo,
    completedExpanded,
    completedSessions.length,
    desiredExpandedHeight,
    notchState.expanded,
  ]);

  const toggleExpanded = async (expanded: boolean) => {
    await api.setNotchExpanded(expanded);
  };
  const togglePinned = async () => {
    await api.setNotchPinned(!notchState.pinned);
  };
  const runImmediateAction = async (key: string, action: () => Promise<void>) => {
    if (pendingActions.current.has(key)) return;
    pendingActions.current.add(key);
    try {
      await action();
    } finally {
      pendingActions.current.delete(key);
    }
  };
  const onActionPointerDown = (
    event: ReactPointerEvent<HTMLButtonElement>,
    key: string,
    action: () => Promise<void>,
  ) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    void runImmediateAction(key, action);
  };
  const onActionClick = (
    event: ReactMouseEvent<HTMLButtonElement>,
    key: string,
    action: () => Promise<void>,
  ) => {
    // Pointer activation is handled on pointerdown so a hover-opened NSPanel
    // cannot swallow the first click. detail=0 preserves keyboard activation.
    if (event.detail !== 0) return;
    event.stopPropagation();
    void runImmediateAction(key, action);
  };
  const jumpToActive = async (id: string) => {
    try {
      await api.jumpToLiveSession(id);
      setActiveJumpError(undefined);
    } catch {
      setActiveJumpError(id);
    }
  };
  const removeCompleted = async (id: string) => {
    await api.deleteNotchCompletedSession(id);
    if (completedJumpError === id) setCompletedJumpError(undefined);
    await snapshot.refetch();
  };
  const jumpToCompleted = async (id: string) => {
    try {
      await api.jumpToNotchCompletedSession(id);
      setCompletedJumpError(undefined);
      await snapshot.refetch();
    } catch {
      setCompletedJumpError(id);
    }
  };
  const clearCompleted = async () => {
    const result = await api.clearNotchCompletedSessions();
    if (!result.count) return;
    setCompletedJumpError(undefined);
    setClearUndo(result);
    if (clearUndoTimeout.current !== undefined) {
      window.clearTimeout(clearUndoTimeout.current);
    }
    clearUndoTimeout.current = window.setTimeout(() => {
      setClearUndo(undefined);
      clearUndoTimeout.current = undefined;
    }, 5_000);
    await snapshot.refetch();
  };
  const undoClearCompleted = async () => {
    if (!clearUndo) return;
    const restored = await api.undoClearNotchCompletedSessions(clearUndo.token);
    if (clearUndoTimeout.current !== undefined) {
      window.clearTimeout(clearUndoTimeout.current);
      clearUndoTimeout.current = undefined;
    }
    setClearUndo(undefined);
    if (restored) await snapshot.refetch();
  };

  if (!notchState.enabled) return null;

  const showExpandedSurface = notchState.expanded || keepExpandedDuringClose;
  const isClosing = showExpandedSurface && !notchState.expanded;

  return (
    <section
      className={`notch-island notch-island-morph ${
        notchState.expanded ? "is-expanded" : isClosing ? "is-closing" : "is-collapsed"
      }${hasActivity ? " has-activity" : " is-idle"}`}
      style={style}
    >
      <button
        className="notch-collapsed-shell"
        onClick={() => void toggleExpanded(true)}
        aria-label={t("notch.open")}
        aria-hidden={showExpandedSurface}
        tabIndex={showExpandedSurface ? -1 : 0}
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
              <ProviderCount agent="kimi-code" count={kimiCodeCount} />
              <ProviderCount agent="zcode" count={zcodeCount} />
            </span>
          )}
        </span>
        <span className="notch-hardware" />
        <span className={`notch-wing notch-wing-right status-${rightSession?.status ?? "idle"}`}>
          {rightSession ? (
            <>
              <AgentActivityGlyph session={rightSession} compact />
              <strong>{notchPhaseLabel(rightSession.phase, t)}</strong>
            </>
          ) : null}
        </span>
      </button>
      {showExpandedSurface ? (
        <div className="notch-island-expanded">
      <header className="notch-expanded-bridge">
        <span className="notch-expanded-left">
          <ProviderCount agent="codex" count={codexCount} />
          <ProviderCount agent="claude-code" count={claudeCount} />
          <ProviderCount agent="kimi-code" count={kimiCodeCount} />
          <ProviderCount agent="zcode" count={zcodeCount} />
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
      <div
        ref={sessionListRef}
        className={`notch-session-list${sessionListScrollable ? " is-scrollable" : ""} ${
          visibleSessions.length || completedSessions.length || clearUndo ? "" : "is-empty"
        }`}
      >
        {visibleSessions.map((session, index) => {
          const reason =
            activeJumpError === session.id ? t("notch.jumpFailed") : liveReason(session, t);
          const conversationTitle = conversationTitleForSession(session);
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
                  <span
                    className={`notch-session-title-line${conversationTitle ? " has-conversation" : ""}`}
                  >
                    <strong>{session.projectLabel}</strong>
                    {conversationTitle ? (
                      <>
                        <i aria-hidden="true">·</i>
                        <span>{conversationTitle}</span>
                      </>
                    ) : null}
                  </span>
                  <small>
                    {agentName(session.agent)} · {t("notch.elapsed", {
                      value: formatLiveElapsed(session.startedAt, liveElapsedEnd(session, now)),
                    })}
                  </small>
                </span>
                <span className="notch-phase">
                  <AgentActivityGlyph session={session} compact />
                  {notchPhaseLabel(session.phase, t)}
                </span>
              </div>
              {reason ? <p>{reason}</p> : null}
              <footer>
                <NotchActionFlow actions={session.actions} t={t} />
                <button
                  onPointerDown={(event) => onActionPointerDown(event, `jump:${session.id}`, () => jumpToActive(session.id))}
                  onClick={(event) => onActionClick(event, `jump:${session.id}`, () => jumpToActive(session.id))}
                  aria-label={t("live.jump")}
                  title={t("live.jump")}
                >
                  <ArrowUpRight size={13} />
                </button>
              </footer>
            </article>
          );
        })}
        {completedSessions.length ? (
          <section className="notch-completed-section">
            <div className="notch-completed-header">
              <button
                className="notch-completed-toggle"
                onClick={() => setCompletedExpanded((expanded) => !expanded)}
                aria-expanded={completedExpanded}
              >
                <ChevronDown
                  size={12}
                  className={completedExpanded ? "is-expanded" : ""}
                  aria-hidden="true"
                />
                <strong>{t("notch.completedCount", { count: completedSessions.length })}</strong>
              </button>
              <button
                className="notch-completed-clear"
                onClick={() => void clearCompleted()}
                aria-label={t("notch.clearCompleted")}
                title={t("notch.clearCompleted")}
              >
                <Trash2 size={12} />
              </button>
            </div>
            {completedExpanded
              ? completedSessions.map((completed, index) => {
                  const session = completed.session;
                  const conversationTitle = conversationTitleForSession(session);
                  return (
                    <article
                      key={session.id}
                      className="status-completed"
                      style={{ "--notch-session-index": visibleSessions.length + index } as CSSProperties}
                    >
                      <div className="notch-session-top">
                        <span className={`notch-agent-dot provider-${session.agent}`}>
                          <ProviderMark agent={session.agent} size={14} />
                        </span>
                        <span>
                          <span
                            className={`notch-session-title-line${conversationTitle ? " has-conversation" : ""}`}
                          >
                            <strong>{session.projectLabel}</strong>
                            {conversationTitle ? (
                              <>
                                <i aria-hidden="true">·</i>
                                <span>{conversationTitle}</span>
                              </>
                            ) : null}
                          </span>
                          <small>
                            {agentName(session.agent)} · {t("notch.totalElapsed", {
                              value: formatLiveElapsed(
                                completed.cycleStartedAt,
                                new Date(completed.completedAt).getTime(),
                              ),
                            })}
                          </small>
                        </span>
                        <span className="notch-phase">
                          <AgentActivityGlyph session={session} compact />
                          {notchPhaseLabel("completed", t)}
                        </span>
                      </div>
                      {completedJumpError === session.id ? (
                        <p>{t("notch.jumpFailed")}</p>
                      ) : null}
                      <footer>
                        <NotchActionFlow actions={session.actions} t={t} />
                        <span className="notch-completed-actions">
                          <button
                            onPointerDown={(event) => onActionPointerDown(event, `remove:${session.id}`, () => removeCompleted(session.id))}
                            onClick={(event) => onActionClick(event, `remove:${session.id}`, () => removeCompleted(session.id))}
                            aria-label={t("notch.removeCompleted")}
                            title={t("notch.removeCompleted")}
                          >
                            <X size={12} />
                          </button>
                          <button
                            onPointerDown={(event) => onActionPointerDown(event, `jump-completed:${session.id}`, () => jumpToCompleted(session.id))}
                            onClick={(event) => onActionClick(event, `jump-completed:${session.id}`, () => jumpToCompleted(session.id))}
                            aria-label={t("live.jump")}
                            title={t("live.jump")}
                          >
                            <ArrowUpRight size={13} />
                          </button>
                        </span>
                      </footer>
                    </article>
                  );
                })
              : null}
          </section>
        ) : null}
        {clearUndo ? (
          <div className="notch-clear-undo" role="status">
            <span>{t("notch.clearedCount", { count: clearUndo.count })}</span>
            <button onClick={() => void undoClearCompleted()}>
              <Undo2 size={12} />
              {t("notch.undoClear")}
            </button>
          </div>
        ) : null}
        {!visibleSessions.length && !completedSessions.length && !clearUndo ? (
          <div className="notch-empty">
            <strong>VibeMeter</strong>
            <span>{t("notch.noActivity")}</span>
          </div>
        ) : null}
      </div>
        </div>
      ) : null}
    </section>
  );
}
