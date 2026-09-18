import { useQuery } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { ArrowUpRight, ChevronLeft, ChevronRight, Gauge, Pin, Settings2, X } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../lib/api";
import { agentName } from "../lib/format";
import {
  edgeAgentQuotas,
  formatBalance,
  formatResetRemaining,
  parseEdgeAgents,
  resetRemainingSeconds,
  resetTime,
  ringCaption,
  type EdgeAgentQuota,
} from "../lib/quota";
import { SIDE_NOTCH_DEPTH, sideNotchPath, sideNotchTransform } from "../lib/sideNotchShape";
import type { EdgeState, Locale, ProviderAccount, RateWindow } from "../types";
import { AgentIcon } from "./AgentIcon";

export const EDGE_FOLD_GRACE_MS = 450;
/** How often the sidebar asks the providers for a fresh reading. */
export const EDGE_QUOTA_REFRESH_MS = 5 * 60_000;
/** How often it re-reads the in-memory snapshot a refresh anywhere leaves. */
const EDGE_SNAPSHOT_POLL_MS = 30_000;

const initialState: EdgeState = {
  enabled: false,
  expanded: false,
  pinned: false,
  side: "right",
  offset: 0.5,
};

/** Movement before a press on the strip counts as a drag and not a click. */
export const EDGE_DRAG_THRESHOLD = 4;

interface DragState {
  pointerId: number;
  /** Where the pointer sat relative to the notch's middle when it went down. */
  grab: number;
  startScreenY: number;
  moved: boolean;
}

const NOTCH_WIDTH = SIDE_NOTCH_DEPTH;
/* Mirrors the block in edge-sidebar.css. The notch is sized in JS because the
   SVG outline needs a number, so the stylesheet follows these rather than the
   other way round. */
const NOTCH_PADDING = 14;
const RING_DIAMETER = 27;
const RING_TRACK_STROKE = 3.6;
const RING_RADIUS = (RING_DIAMETER - RING_TRACK_STROKE) / 2;
const RING_CIRCUMFERENCE = 2 * Math.PI * RING_RADIUS;
/** Ring plus the gap and line box of its percentage caption. */
const RING_CELL_HEIGHT = RING_DIAMETER + 4 + 10;
const RING_GAP = 12;
/** The flare owns this much of each end, where the shape has left the body. */
const NOTCH_CURL = 24;
/** The folded panel in `edge.rs` is this tall; the notch cannot outgrow it. */
const NOTCH_MAX_HEIGHT = 720;

export function notchHeightFor(ringCount: number): number {
  const rings = Math.max(1, ringCount);
  const content =
    NOTCH_CURL * 2 +
    NOTCH_PADDING * 2 +
    rings * RING_CELL_HEIGHT +
    (rings - 1) * RING_GAP;
  return Math.min(NOTCH_MAX_HEIGHT, content);
}

function SideNotchPath({
  side,
  height,
  width = NOTCH_WIDTH,
}: {
  side: "left" | "right";
  height: number;
  width?: number;
}) {
  const d = sideNotchPath({ depth: width, length: height });
  return (
    <svg
      className="edge-notch-svg"
      width={width}
      height={height}
      viewBox={`0 0 ${width} ${height}`}
      aria-hidden="true"
    >
      <g transform={sideNotchTransform(side, width)}>
        <path d={d} className="edge-notch-fill" />
        <path d={d} className="edge-notch-stroke" />
      </g>
    </svg>
  );
}

function QuotaWindowRow({
  window,
  locale,
  now,
}: {
  window: RateWindow;
  locale: Locale;
  now: Date;
}) {
  const { t } = useTranslation();
  const used = window.usedPercent;
  const remaining = used === undefined ? undefined : Math.max(0, Math.min(100, 100 - used));
  const band =
    remaining === undefined
      ? "unknown"
      : remaining < 20
        ? "critical"
        : remaining < 50
          ? "warning"
          : "steady";
  const reset = resetTime(window, locale);
  const resetSeconds = resetRemainingSeconds(window, now);
  const resetLabel = reset ? t("quota.resetsAt", { time: reset }) : t("quota.resetUnknown");
  const countdown =
    resetSeconds === undefined
      ? undefined
      : t("quota.resetIn", { time: formatResetRemaining(resetSeconds, locale) });
  return (
    <article className={`edge-quota-window band-${band}`}>
      <div className="edge-quota-heading">
        <strong>{t(window.label, { defaultValue: window.label })}</strong>
        <span>
          {remaining === undefined
            ? t("edge.unavailable")
            : t("quota.remaining", { value: Math.round(remaining) })}
        </span>
      </div>
      <div className="edge-quota-track" aria-hidden="true">
        <i style={{ width: `${remaining ?? 0}%` }} />
      </div>
      <p className="edge-quota-reset">
        {resetLabel}
        {countdown ? <span> · {countdown}</span> : null}
      </p>
    </article>
  );
}

/**
 * One credential's worth of a provider. A balance gets no bar and no reset:
 * there is no denominator to fill and nothing to count down to.
 */
function AccountBlock({
  account,
  locale,
  now,
}: {
  account: ProviderAccount;
  locale: Locale;
  now: Date;
}) {
  const { t } = useTranslation();
  const balance = account.balance ?? undefined;
  return (
    <section className="edge-account">
      <header className="edge-account-name">
        <strong title={account.label}>{account.label}</strong>
        <em>{t(account.kind === "api" ? "edge.kindApi" : "edge.kindSubscription")}</em>
      </header>
      {!account.available ? (
        <p className="edge-account-note">
          {t(account.errorKey ?? "edge.unavailable", { defaultValue: t("edge.unavailable") })}
        </p>
      ) : balance ? (
        <article className={`edge-balance ${balance.spendable === false ? "is-spent" : ""}`}>
          <div className="edge-quota-heading">
            <strong>{t("edge.balance")}</strong>
            <span>{formatBalance(balance, locale)}</span>
          </div>
          <p className="edge-quota-reset">
            {balance.spendable === false
              ? t("edge.balanceBlocked")
              : balance.granted !== undefined
                ? t("edge.balanceSplit", {
                    granted: formatBalance({ ...balance, total: balance.granted }, locale),
                    toppedUp: formatBalance(
                      { ...balance, total: balance.toppedUp ?? 0 },
                      locale,
                    ),
                  })
                : t("edge.balanceNoReset")}
          </p>
        </article>
      ) : account.windows.length ? (
        account.windows.map((window) => (
          <QuotaWindowRow key={window.id} window={window} locale={locale} now={now} />
        ))
      ) : (
        <p className="edge-account-note">{t("edge.unavailable")}</p>
      )}
    </section>
  );
}

export function EdgeSidebar({ locale }: { locale: Locale }) {
  const { t } = useTranslation();
  const [state, setState] = useState(initialState);
  const [selected, setSelected] = useState<string>();
  const [commandError, setCommandError] = useState(false);
  const [now, setNow] = useState(() => new Date());
  const [dragging, setDragging] = useState(false);
  const stateRef = useRef(state);
  const notchRef = useRef<HTMLElement>(null);
  const drag = useRef<DragState | undefined>(undefined);
  /* What was last asked of the panel, which runs ahead of what it reports.
     Sweeping the rings fires an enter per ring, and each unguarded ask is
     another native resize and reposition for a panel that is already open. */
  const requested = useRef<boolean | undefined>(undefined);
  const foldTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const panelRef = useRef<HTMLElement>(null);
  const sheetRef = useRef<HTMLElement>(null);
  const tailRef = useRef<HTMLSpanElement>(null);
  const ringRefs = useRef<Map<string, HTMLButtonElement>>(new Map());

  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  const quota = useQuery({
    queryKey: ["edge-quota"],
    queryFn: api.providers,
    refetchInterval: EDGE_SNAPSHOT_POLL_MS,
  });
  const sources = useQuery({ queryKey: ["sources"], queryFn: api.sources });

  const cancelFold = () => {
    clearTimeout(foldTimer.current);
  };
  const updateState = (next: EdgeState) => {
    stateRef.current = next;
    requested.current = next.expanded;
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
    const timer = setInterval(() => setNow(new Date()), 30_000);
    return () => {
      disposed = true;
      cleanup?.();
      clearInterval(timer);
      clearTimeout(foldTimer.current);
    };
  }, []);

  const credentialsAllowed = settings.data?.credentialsAllowed === "true";
  const cursorDashboardUsage = settings.data?.cursorDashboardUsage === "true";
  const useSystemProxy = settings.data?.useSystemProxy === "true";

  // Quota is a network reading, not a local file: it only moves when someone
  // asks. Keep asking on the sidebar's own clock so a pinned panel does not sit
  // on whatever the menu bar last happened to fetch.
  useEffect(() => {
    if (!settings.data || !credentialsAllowed) return;
    let disposed = false;
    const refresh = () => {
      void api
        .refreshProviders(true, cursorDashboardUsage, useSystemProxy)
        .then(() => {
          if (!disposed) void quota.refetch();
        })
        .catch(() => undefined);
    };
    refresh();
    const timer = setInterval(refresh, EDGE_QUOTA_REFRESH_MS);
    return () => {
      disposed = true;
      clearInterval(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [Boolean(settings.data), credentialsAllowed, cursorDashboardUsage, useSystemProxy]);

  const expand = async (expanded: boolean, pinned?: boolean) => {
    cancelFold();
    setCommandError(false);
    if (pinned === undefined && requested.current === expanded) return;
    requested.current = expanded;
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

  const configuredAgents = parseEdgeAgents(settings.data?.edgeSidebarAgents);
  const rings: EdgeAgentQuota[] = edgeAgentQuotas(
    sources.data ?? [],
    quota.data ?? [],
    configuredAgents,
  );
  /* A chosen-but-empty list is the user asking for none, which is a different
     thing from nothing being detected. */
  const hiddenByChoice = Boolean(configuredAgents) && !rings.length;
  const active = rings.find((ring) => ring.agent === selected) ?? rings[0];
  const summary = active?.summary;

  const openMain = async (settingsPage = false) => {
    try {
      if (settingsPage) await api.showSettings();
      else await api.showMain();
    } catch {
      setCommandError(true);
    }
  };

  const notchHeight = notchHeightFor(rings.length);

  // Point the tail at the ring it belongs to by measuring both, so it keeps up
  // with a list that grows or shrinks between readings. Written straight to the
  // node: as state this would cost a second render on every hover, which is
  // exactly when the sidebar can least afford one.
  useLayoutEffect(() => {
    const sheet = sheetRef.current;
    const tail = tailRef.current;
    const ring = active ? ringRefs.current.get(active.agent) : undefined;
    if (!sheet || !tail || !ring) return;
    const sheetRect = sheet.getBoundingClientRect();
    const ringRect = ring.getBoundingClientRect();
    if (!sheetRect.height || !ringRect.height) return;
    const center = ringRect.top + RING_DIAMETER / 2 - sheetRect.top;
    const clamped = Math.min(Math.max(center, 16), sheetRect.height - 16);
    tail.style.top = `${Math.round(clamped)}px`;
  }, [active, rings.length, state.expanded, state.side]);

  useEffect(() => {
    void api.setEdgePlacement(undefined, notchHeight).catch(() => undefined);
  }, [notchHeight]);

  /* Dragging moves the panel, so the pointer stays over the strip the whole
     time; capture keeps the events coming even when a frame lands late and the
     cursor is briefly off it. The offset is only written back to settings once
     the drag ends — one row per drag, not one per frame. */
  const startDrag = (event: React.PointerEvent<HTMLElement>) => {
    if (event.button !== 0) return;
    const notch = notchRef.current;
    if (!notch) return;
    const rect = notch.getBoundingClientRect();
    const center = window.screenY + rect.top + rect.height / 2;
    drag.current = {
      pointerId: event.pointerId,
      grab: event.screenY - center,
      startScreenY: event.screenY,
      moved: false,
    };
    notch.setPointerCapture(event.pointerId);
  };

  const moveDrag = (event: React.PointerEvent<HTMLElement>) => {
    const current = drag.current;
    if (!current || current.pointerId !== event.pointerId) return;
    if (!current.moved) {
      if (Math.abs(event.screenY - current.startScreenY) < EDGE_DRAG_THRESHOLD) return;
      current.moved = true;
      setDragging(true);
      cancelFold();
    }
    void api.setEdgePlacement(event.screenY - current.grab, notchHeight).catch(() => undefined);
  };

  const endDrag = (event: React.PointerEvent<HTMLElement>) => {
    const current = drag.current;
    if (!current || current.pointerId !== event.pointerId) return;
    drag.current = undefined;
    notchRef.current?.releasePointerCapture(event.pointerId);
    if (!current.moved) return;
    setDragging(false);
    void api.setSetting("edgeSidebarOffset", String(stateRef.current.offset)).catch(() => undefined);
  };

  const select = (agent: string) => {
    setSelected(agent);
    if (!stateRef.current.expanded) void expand(true);
  };

  const refreshedAt = active?.provider?.refreshedAt
    ? new Date(active.provider.refreshedAt)
    : undefined;

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
      <aside
        ref={notchRef}
        className={`edge-notch ${dragging ? "is-dragging" : ""}`}
        style={{ height: notchHeight }}
        title={t("edge.dragHint")}
        onPointerDown={startDrag}
        onPointerMove={moveDrag}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
      >
        <SideNotchPath
          side={state.side === "left" ? "left" : "right"}
          height={notchHeight}
        />
        <div className="edge-notch-body">
          <div className="edge-provider-rings">
            {rings.map((ring) => {
              const info = ring.summary;
              const isSelected = active?.agent === ring.agent;
              const remaining = info.remainingPercent;
              const name = agentName(ring.agent);
              const label = ringCaption(info, locale);
              const dashOffset =
                RING_CIRCUMFERENCE * (1 - Math.min(Math.max((remaining ?? 0) / 100, 0), 1));
              return (
                <button
                  key={ring.agent}
                  ref={(el) => {
                    if (el) ringRefs.current.set(ring.agent, el);
                    else ringRefs.current.delete(ring.agent);
                  }}
                  className={`edge-ring-cell ${isSelected ? "is-selected" : ""} band-${info.band}`}
                  title={name}
                  aria-label={
                    remaining === undefined
                      ? `${name} · ${label === "—" ? t("edge.unavailable") : label}`
                      : `${name} · ${t("quota.remaining", { value: Math.round(remaining) })}`
                  }
                  aria-pressed={isSelected}
                  onPointerEnter={() => {
                    if (!drag.current?.moved) select(ring.agent);
                  }}
                  onFocus={() => select(ring.agent)}
                  onClick={() => {
                    if (!drag.current?.moved) select(ring.agent);
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
                        cx={RING_DIAMETER / 2}
                        cy={RING_DIAMETER / 2}
                        r={RING_RADIUS}
                        className="edge-ring-track"
                      />
                      {remaining === undefined ? null : (
                        <circle
                          cx={RING_DIAMETER / 2}
                          cy={RING_DIAMETER / 2}
                          r={RING_RADIUS}
                          className={`edge-ring-arc arc-${info.band}`}
                          style={{
                            strokeDasharray: RING_CIRCUMFERENCE,
                            strokeDashoffset: dashOffset,
                          }}
                        />
                      )}
                    </svg>
                    <div className="edge-ring-icon">
                      <AgentIcon agent={ring.agent} size={11} />
                    </div>
                  </div>
                  <span className="edge-ring-label">{label}</span>
                </button>
              );
            })}
          </div>
        </div>
      </aside>

      {/* CodeNotch Floating TooltipCard */}
      <section
        ref={sheetRef}
        className="edge-sheet"
        inert={!state.expanded}
        aria-hidden={!state.expanded}
      >
        <span
          ref={tailRef}
          className={`edge-tooltip-tail tail-${state.side}`}
          aria-hidden="true"
        />
        <div className="edge-detail">
          <header className="edge-card-header">
            <div className="edge-card-title">
              {active ? <AgentIcon agent={active.agent} size={15} /> : <Gauge size={15} />}
              <h1 title={active ? agentName(active.agent) : undefined}>
                {active ? agentName(active.agent) : t("edge.quota")}
              </h1>
            </div>
            <div className="edge-card-actions">
              <button
                aria-label={t("edge.settings")}
                onClick={() => void openMain(true)}
                title={t("edge.settings")}
              >
                <Settings2 size={12} />
              </button>
              <button
                aria-label={t(state.pinned ? "edge.unpin" : "edge.pin")}
                aria-pressed={state.pinned}
                className={state.pinned ? "selected" : ""}
                onClick={() => void expand(true, !state.pinned)}
                title={t(state.pinned ? "edge.unpin" : "edge.pin")}
              >
                <Pin size={12} />
              </button>
              <button
                aria-label={t("edge.close")}
                onClick={() => {
                  (document.activeElement as HTMLElement)?.blur();
                  void expand(false, false);
                }}
                title={t("edge.close")}
              >
                <X size={12} />
              </button>
            </div>
          </header>

          <div className="edge-quota-list">
            {!credentialsAllowed && settings.data ? (
              <button className="edge-enable-quota" onClick={() => void openMain(true)}>
                <span>
                  <strong>{t("quota.noQuota")}</strong>
                  <small>{t("quota.enableInSettings")}</small>
                </span>
                <ArrowUpRight size={13} />
              </button>
            ) : quota.isLoading || settings.isLoading ? (
              <p className="edge-empty" role="status">
                {t("edge.loading")}
              </p>
            ) : quota.isError ? (
              <div className="edge-empty" role="alert">
                <strong>{t("edge.error")}</strong>
                <button onClick={() => void quota.refetch()}>{t("edge.retry")}</button>
              </div>
            ) : hiddenByChoice ? (
              <button className="edge-enable-quota" onClick={() => void openMain(true)}>
                <span>
                  <strong>{t("edge.allHidden")}</strong>
                  <small>{t("edge.allHiddenBody")}</small>
                </span>
                <ArrowUpRight size={13} />
              </button>
            ) : !active ? (
              <div className="edge-empty">
                <Gauge size={22} />
                <strong>{t("edge.empty")}</strong>
                <p>{t("edge.emptyBody")}</p>
              </div>
            ) : !active.accounts.length ? (
              <div className="edge-empty">
                <Gauge size={22} />
                <strong>{t("edge.unavailable")}</strong>
                <p>{t(active.provider ? "edge.unavailableBody" : "edge.noSubscriptionBody")}</p>
              </div>
            ) : (
              active.accounts.map((account) => (
                <AccountBlock account={account} key={account.id} locale={locale} now={now} />
              ))
            )}
            {active?.provider?.stale ? <p className="edge-note">{t("edge.stale")}</p> : null}
          </div>

          {commandError ? (
            <p className="edge-error" role="alert">
              {t("edge.error")}
            </p>
          ) : null}

          <footer>
            <span>
              {refreshedAt && !Number.isNaN(refreshedAt.getTime())
                ? t("edge.refreshedAt", {
                    time: new Intl.DateTimeFormat(locale, {
                      hour: "2-digit",
                      minute: "2-digit",
                    }).format(refreshedAt),
                  })
                : t("edge.quota")}
            </span>
            <button onClick={() => void openMain()}>
              {t("edge.overview")}
              {state.side === "left" ? <ChevronRight size={11} /> : <ChevronLeft size={11} />}
            </button>
          </footer>
        </div>
      </section>
    </main>
  );
}
