import { listen } from "@tauri-apps/api/event";
import { Activity, ArrowUpRight, ChevronLeft, ChevronRight, Pin, Settings2, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../lib/api";
import { agentName } from "../lib/format";
import { useLiveSnapshot } from "../lib/useLiveSnapshot";
import type { EdgeState, LiveSession, Locale } from "../types";
import { AgentIcon } from "./AgentIcon";
import { formatLiveElapsed, liveElapsedEnd, notchPulseValue } from "./NotchSurface";

export const EDGE_FOLD_GRACE_MS = 450;
const initialState: EdgeState = { enabled: false, expanded: false, pinned: false, side: "right" };

const NOTCH_WIDTH = 68;
const RING_DIAMETER = 44;
const RING_RADIUS = 19;
const RING_CIRCUMFERENCE = 2 * Math.PI * RING_RADIUS;

function SideNotchPath({
  side,
  height,
  width = NOTCH_WIDTH,
}: {
  side: "left" | "right";
  height: number;
  width?: number;
}) {
  const curl = 24;
  const corner = 18;
  const h = Math.max(height, 160);
  const w = width;

  const d =
    side === "right"
      ? `M ${w} 0 A ${curl} ${curl} 0 0 0 ${w - curl} ${curl} L ${corner} ${curl} A ${corner} ${corner} 0 0 0 0 ${curl + corner} L 0 ${h - curl - corner} A ${corner} ${corner} 0 0 0 ${corner} ${h - curl} L ${w - curl} ${h - curl} A ${curl} ${curl} 0 0 0 ${w} ${h} Z`
      : `M 0 0 A ${curl} ${curl} 0 0 0 ${curl} ${curl} L ${w - corner} ${curl} A ${corner} ${corner} 0 0 0 ${w} ${curl + corner} L ${w} ${h - curl - corner} A ${corner} ${corner} 0 0 0 ${w - corner} ${h - curl} L ${curl} ${h - curl} A ${curl} ${curl} 0 0 0 0 ${h} Z`;

  return (
    <svg
      className="edge-notch-svg"
      width={w}
      height={h}
      viewBox={`0 0 ${w} ${h}`}
      aria-hidden="true"
    >
      <path d={d} className="edge-notch-fill" />
      <path d={d} className="edge-notch-stroke" />
    </svg>
  );
}

function providerStatus(sessions: LiveSession[]): {
  status: "running" | "waiting" | "error" | "idle";
  fraction: number;
  label: string;
} {
  if (!sessions.length) {
    return { status: "idle", fraction: 0, label: "idle" };
  }
  const hasError = sessions.some(
    (s) => s.status === "error" || s.pulse.lifecycle.value === "error",
  );
  if (hasError) {
    return { status: "error", fraction: 1, label: "err" };
  }
  const hasWaiting = sessions.some(
    (s) => s.status === "waiting" || s.pulse.lifecycle.value === "waiting",
  );
  if (hasWaiting) {
    return { status: "waiting", fraction: 0.85, label: "wait" };
  }
  const hasRunning = sessions.some(
    (s) => s.status === "running" || s.pulse.lifecycle.value === "running",
  );
  if (hasRunning) {
    return { status: "running", fraction: 0.72, label: `${sessions.length} act` };
  }
  return { status: "idle", fraction: 0.2, label: "idle" };
}

export function EdgeSidebar({ locale: _locale }: { locale: Locale }) {
  const { t } = useTranslation();
  const snapshot = useLiveSnapshot();
  const [state, setState] = useState(initialState);
  const [provider, setProvider] = useState<LiveSession["agent"]>();
  const [jumpError, setJumpError] = useState<string>();
  const [commandError, setCommandError] = useState(false);
  const [pending, setPending] = useState<string>();
  const [now, setNow] = useState(Date.now());
  const stateRef = useRef(state);
  const foldTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const panelRef = useRef<HTMLElement>(null);
  const ringRefs = useRef<Map<string, HTMLButtonElement>>(new Map());

  const cancelFold = () => {
    clearTimeout(foldTimer.current);
  };
  const updateState = (next: EdgeState) => {
    stateRef.current = next;
    setState(next);
  };

  useEffect(() => {
    let disposed = false;
    let cleanup: (() => void) | undefined;
    void listen<EdgeState>("edge-state", ({ payload }) => {
      if (!disposed) updateState(payload);
    })
      .then(async (unlisten) => {
        if (disposed) {
          unlisten();
          return;
        }
        cleanup = unlisten;
        const current = await api.edgeState();
        if (!disposed) updateState(current);
      })
      .catch(() => {
        if (!disposed) setCommandError(true);
      });
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => {
      disposed = true;
      cleanup?.();
      clearInterval(timer);
      clearTimeout(foldTimer.current);
    };
  }, []);

  const expand = async (expanded: boolean, pinned?: boolean) => {
    cancelFold();
    setCommandError(false);
    try {
      await api.setEdgeExpanded(expanded, pinned);
    } catch {
      setCommandError(true);
    }
  };

  const scheduleFold = () => {
    cancelFold();
    if (stateRef.current.pinned) return;
    foldTimer.current = setTimeout(() => {
      if (
        !stateRef.current.pinned &&
        !(
          panelRef.current?.contains(document.activeElement) &&
          document.activeElement?.matches(":focus-visible")
        )
      ) {
        void expand(false);
      }
    }, EDGE_FOLD_GRACE_MS);
  };

  const sessions = snapshot.data?.sessions ?? [];
  const providers = useMemo(
    () => [...new Set(sessions.map((session) => session.agent))],
    [sessions],
  );
  const selectedProvider =
    provider && providers.includes(provider) ? provider : undefined;
  const visible = sessions.filter(
    (session) => !selectedProvider || session.agent === selectedProvider,
  );

  const jump = async (session: LiveSession) => {
    if (pending) return;
    setPending(session.id);
    setJumpError(undefined);
    try {
      await api.jumpToLiveSession(session.id);
    } catch {
      setJumpError(session.id);
    } finally {
      setPending(undefined);
    }
  };

  const openMain = async (settings = false) => {
    try {
      if (settings) await api.showSettings();
      else await api.showMain();
    } catch {
      setCommandError(true);
    }
  };

  const notchHeight = Math.max(180, 56 + Math.max(1, providers.length) * 66 + 36);

  const selectedIndex = selectedProvider ? providers.indexOf(selectedProvider) : -1;
  const tailTop =
    selectedIndex >= 0
      ? 28 + selectedIndex * 66 + 22 - 7
      : 50;

  return (
    <main
      ref={panelRef}
      className={`edge-surface side-${state.side} ${state.expanded ? "is-expanded" : "is-folded"}`}
      aria-label={t("edge.title")}
      onPointerEnter={() => {
        cancelFold();
        if (!stateRef.current.expanded) void expand(true);
      }}
      onPointerLeave={scheduleFold}
      onFocus={cancelFold}
      onBlur={scheduleFold}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          (document.activeElement as HTMLElement)?.blur();
          void expand(false, false);
        }
      }}
    >
      {/* CodeNotch Docked Edge Notch */}
      <aside className="edge-notch" style={{ height: notchHeight }}>
        <SideNotchPath
          side={state.side === "left" ? "left" : "right"}
          height={notchHeight}
        />
        <div className="edge-notch-body">
          <div className="edge-provider-rings">
            {providers.map((agent) => {
              const agentSessions = sessions.filter((s) => s.agent === agent);
              const info = providerStatus(agentSessions);
              const isSelected = selectedProvider === agent;
              const dashOffset =
                RING_CIRCUMFERENCE * (1 - Math.min(Math.max(info.fraction, 0), 1));
              return (
                <button
                  key={agent}
                  ref={(el) => {
                    if (el) ringRefs.current.set(agent, el);
                    else ringRefs.current.delete(agent);
                  }}
                  className={`edge-ring-cell ${isSelected ? "is-selected" : ""} status-${info.status}`}
                  title={agentName(agent)}
                  aria-label={agentName(agent)}
                  aria-pressed={isSelected}
                  onPointerEnter={() => {
                    setProvider(agent);
                    if (!stateRef.current.expanded) void expand(true);
                  }}
                  onFocus={() => {
                    setProvider(agent);
                    if (!stateRef.current.expanded) void expand(true);
                  }}
                  onClick={() => {
                    setProvider(agent);
                    if (!stateRef.current.expanded) void expand(true);
                  }}
                >
                  <div className="edge-ring-wrapper">
                    <svg
                      className="edge-ring-svg"
                      width={RING_DIAMETER}
                      height={RING_DIAMETER}
                      viewBox={`0 0 ${RING_DIAMETER} ${RING_DIAMETER}`}
                    >
                      <circle
                        cx="22"
                        cy="22"
                        r={RING_RADIUS}
                        className="edge-ring-track"
                      />
                      <circle
                        cx="22"
                        cy="22"
                        r={RING_RADIUS}
                        className={`edge-ring-arc arc-${info.status}`}
                        style={{
                          strokeDasharray: RING_CIRCUMFERENCE,
                          strokeDashoffset: dashOffset,
                        }}
                      />
                    </svg>
                    <div className="edge-ring-icon">
                      <AgentIcon agent={agent} size={20} />
                    </div>
                  </div>
                  <span className="edge-ring-label">{info.label}</span>
                </button>
              );
            })}
          </div>
          <button
            className="edge-notch-settings"
            title={t("edge.settings")}
            aria-label={t("edge.settings")}
            onClick={() => void openMain(true)}
          >
            <Settings2 size={16} />
          </button>
        </div>
      </aside>

      {/* CodeNotch Floating TooltipCard */}
      <section
        className="edge-sheet"
        inert={!state.expanded}
        aria-hidden={!state.expanded}
      >
        <span
          className={`edge-tooltip-tail tail-${state.side}`}
          style={{ top: `${tailTop}px` }}
          aria-hidden="true"
        />
        <div className="edge-detail">
          <header className="edge-card-header">
            <div className="edge-card-title">
              {selectedProvider ? (
                <AgentIcon agent={selectedProvider} size={18} />
              ) : (
                <Activity size={18} />
              )}
              <h1>{selectedProvider ? agentName(selectedProvider) : t("edge.recent")}</h1>
            </div>
            <div className="edge-card-actions">
              <button
                aria-label={t(state.pinned ? "edge.unpin" : "edge.pin")}
                aria-pressed={state.pinned}
                className={state.pinned ? "selected" : ""}
                onClick={() => void expand(true, !state.pinned)}
                title={t(state.pinned ? "edge.unpin" : "edge.pin")}
              >
                <Pin size={14} />
              </button>
              <button
                aria-label={t("edge.close")}
                onClick={() => {
                  (document.activeElement as HTMLElement)?.blur();
                  void expand(false, false);
                }}
                title={t("edge.close")}
              >
                <X size={14} />
              </button>
            </div>
          </header>

          <div className="edge-session-list" key={selectedProvider ?? "all"}>
            {snapshot.isLoading ? (
              <p className="edge-empty" role="status">
                {t("edge.loading")}
              </p>
            ) : snapshot.isError ? (
              <div className="edge-empty" role="alert">
                <strong>{t("edge.error")}</strong>
                <button onClick={() => void snapshot.refetch()}>
                  {t("edge.retry")}
                </button>
              </div>
            ) : !visible.length ? (
              <div className="edge-empty">
                <Activity size={28} />
                <strong>{t("edge.empty")}</strong>
                <p>{t("edge.emptyBody")}</p>
              </div>
            ) : (
              visible.map((session, index) => {
                const exact = session.pulse.lifecycle.availability === "available";
                const lifecycle = exact
                  ? session.pulse.lifecycle.value ?? "idle"
                  : "unknown";
                const pulse = notchPulseValue(session);
                return (
                  <article
                    key={session.id}
                    className={`edge-session status-${lifecycle}`}
                    style={{ "--edge-index": index } as CSSProperties}
                  >
                    <div className="edge-session-heading">
                      <AgentIcon agent={session.agent} size={14} />
                      <strong title={session.projectLabel}>
                        {session.projectLabel}
                      </strong>
                      <span className="edge-status-dot" />
                    </div>
                    <h2>{session.conversationTitle || session.projectLabel}</h2>
                    <div className="edge-session-meta">
                      <span>
                        {exact
                          ? t(`live.status.${lifecycle}`, {
                              defaultValue: t("edge.unavailable"),
                            })
                          : t("edge.unavailable")}
                      </span>
                      <time>
                        {formatLiveElapsed(
                          session.startedAt,
                          liveElapsedEnd(session, now),
                        )}
                      </time>
                    </div>
                    <p className="edge-phase">
                      {t(`notch.phase.${pulse}`, {
                        defaultValue: t(`live.pulse.value.${pulse}`, {
                          defaultValue: t("edge.unavailable"),
                        }),
                      })}
                    </p>
                    <button
                      className="edge-jump"
                      disabled={Boolean(pending)}
                      onClick={() => void jump(session)}
                    >
                      {t("edge.jump")}
                      <ArrowUpRight size={13} />
                    </button>
                    {jumpError === session.id ? (
                      <p className="edge-error" role="alert">
                        {t("edge.jumpError")}
                      </p>
                    ) : null}
                  </article>
                );
              })
            )}
          </div>

          {commandError ? (
            <p className="edge-error" role="alert">
              {t("edge.error")}
            </p>
          ) : null}

          <footer>
            <span>{t("edge.count", { count: visible.length })}</span>
            <button onClick={() => void openMain()}>
              {t("edge.overview")}
              {state.side === "left" ? (
                <ChevronRight size={13} />
              ) : (
                <ChevronLeft size={13} />
              )}
            </button>
          </footer>
        </div>
      </section>
    </main>
  );
}
