import { useQuery } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { ArrowUpRight, ChevronLeft, ChevronRight, Gauge, Pin, Settings2, X } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../lib/api";
import {
  formatResetRemaining,
  providerAgentIcon,
  providerDisplayName,
  quotaSummary,
  resetRemainingSeconds,
  resetTime,
  type QuotaSummary,
} from "../lib/quota";
import { sideNotchPath, sideNotchTransform } from "../lib/sideNotchShape";
import type { EdgeState, Locale, ProviderUsage } from "../types";
import { AgentIcon } from "./AgentIcon";

export const EDGE_FOLD_GRACE_MS = 450;
/** How often the sidebar asks the providers for a fresh reading. */
export const EDGE_QUOTA_REFRESH_MS = 5 * 60_000;
/** How often it re-reads the in-memory snapshot a refresh anywhere leaves. */
const EDGE_SNAPSHOT_POLL_MS = 30_000;

const initialState: EdgeState = { enabled: false, expanded: false, pinned: false, side: "right" };

const NOTCH_WIDTH = 68;
/** Keeps the ring column clear of the flare at both ends. */
const NOTCH_PADDING = 30;
const RING_DIAMETER = 44;
const RING_RADIUS = 19;
const RING_CIRCUMFERENCE = 2 * Math.PI * RING_RADIUS;
/** Ring plus its percentage caption. */
const RING_CELL_HEIGHT = RING_DIAMETER + 4 + 11;
const RING_GAP = 14;
const NOTCH_FOOTER_HEIGHT = 14 + 28;
/** The folded panel in `edge.rs` is this tall; the notch cannot outgrow it. */
const NOTCH_MAX_HEIGHT = 320;
const NOTCH_MIN_HEIGHT = 180;

export function notchHeightFor(ringCount: number): number {
  const rings = Math.max(1, ringCount);
  const content =
    NOTCH_PADDING * 2 +
    rings * RING_CELL_HEIGHT +
    (rings - 1) * RING_GAP +
    NOTCH_FOOTER_HEIGHT;
  return Math.min(NOTCH_MAX_HEIGHT, Math.max(NOTCH_MIN_HEIGHT, content));
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

export function EdgeSidebar({ locale }: { locale: Locale }) {
  const { t } = useTranslation();
  const [state, setState] = useState(initialState);
  const [selected, setSelected] = useState<string>();
  const [commandError, setCommandError] = useState(false);
  const [now, setNow] = useState(() => new Date());
  const [tailTop, setTailTop] = useState(48);
  const stateRef = useRef(state);
  const foldTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const panelRef = useRef<HTMLElement>(null);
  const sheetRef = useRef<HTMLElement>(null);
  const ringRefs = useRef<Map<string, HTMLButtonElement>>(new Map());

  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  const quota = useQuery({
    queryKey: ["edge-quota"],
    queryFn: api.providers,
    refetchInterval: EDGE_SNAPSHOT_POLL_MS,
  });

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

  const providers: ProviderUsage[] = quota.data ?? [];
  const active =
    providers.find((provider) => provider.provider === selected) ?? providers[0];
  const summary: QuotaSummary | undefined = active ? quotaSummary(active) : undefined;

  const openMain = async (settingsPage = false) => {
    try {
      if (settingsPage) await api.showSettings();
      else await api.showMain();
    } catch {
      setCommandError(true);
    }
  };

  const notchHeight = notchHeightFor(providers.length);

  // Point the tail at the ring it belongs to by measuring both, so it keeps up
  // with a provider list that grows or shrinks between readings.
  useLayoutEffect(() => {
    const sheet = sheetRef.current;
    const ring = active ? ringRefs.current.get(active.provider) : undefined;
    if (!sheet || !ring) return;
    const sheetRect = sheet.getBoundingClientRect();
    const ringRect = ring.getBoundingClientRect();
    if (!sheetRect.height || !ringRect.height) return;
    const center = ringRect.top + RING_DIAMETER / 2 - sheetRect.top;
    setTailTop(Math.round(Math.min(Math.max(center, 18), sheetRect.height - 18)));
  }, [active, providers.length, state.expanded, state.side]);

  const select = (provider: string) => {
    setSelected(provider);
    if (!stateRef.current.expanded) void expand(true);
  };

  const refreshedAt = active?.refreshedAt ? new Date(active.refreshedAt) : undefined;

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
            {providers.map((provider) => {
              const info = quotaSummary(provider);
              const isSelected = active?.provider === provider.provider;
              const remaining = info.remainingPercent;
              const name = providerDisplayName(provider.provider);
              const label =
                remaining === undefined ? "—" : `${Math.round(remaining)}%`;
              const dashOffset =
                RING_CIRCUMFERENCE * (1 - Math.min(Math.max((remaining ?? 0) / 100, 0), 1));
              return (
                <button
                  key={provider.provider}
                  ref={(el) => {
                    if (el) ringRefs.current.set(provider.provider, el);
                    else ringRefs.current.delete(provider.provider);
                  }}
                  className={`edge-ring-cell ${isSelected ? "is-selected" : ""} band-${info.band}`}
                  title={name}
                  aria-label={
                    remaining === undefined
                      ? `${name} · ${t("edge.unavailable")}`
                      : `${name} · ${t("quota.remaining", { value: Math.round(remaining) })}`
                  }
                  aria-pressed={isSelected}
                  onPointerEnter={() => select(provider.provider)}
                  onFocus={() => select(provider.provider)}
                  onClick={() => select(provider.provider)}
                >
                  <div className="edge-ring-wrapper">
                    <svg
                      className="edge-ring-svg"
                      width={RING_DIAMETER}
                      height={RING_DIAMETER}
                      viewBox={`0 0 ${RING_DIAMETER} ${RING_DIAMETER}`}
                    >
                      <circle cx="22" cy="22" r={RING_RADIUS} className="edge-ring-track" />
                      {remaining === undefined ? null : (
                        <circle
                          cx="22"
                          cy="22"
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
                      <AgentIcon agent={providerAgentIcon(provider.provider)} size={20} />
                    </div>
                  </div>
                  <span className="edge-ring-label">{label}</span>
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
        ref={sheetRef}
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
              {active ? (
                <AgentIcon agent={providerAgentIcon(active.provider)} size={18} />
              ) : (
                <Gauge size={18} />
              )}
              <h1>{active ? providerDisplayName(active.provider) : t("edge.quota")}</h1>
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

          <div className="edge-quota-list" key={active?.provider ?? "none"}>
            {!credentialsAllowed && settings.data ? (
              <button className="edge-enable-quota" onClick={() => void openMain(true)}>
                <span>
                  <strong>{t("quota.noQuota")}</strong>
                  <small>{t("quota.enableInSettings")}</small>
                </span>
                <ArrowUpRight size={15} />
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
            ) : !active ? (
              <div className="edge-empty">
                <Gauge size={28} />
                <strong>{t("edge.empty")}</strong>
                <p>{t("edge.emptyBody")}</p>
              </div>
            ) : !summary?.windows.length ? (
              <div className="edge-empty">
                <Gauge size={28} />
                <strong>{t("edge.unavailable")}</strong>
                <p>{t("edge.unavailableBody")}</p>
              </div>
            ) : (
              summary.windows.map((window) => {
                const used = window.usedPercent;
                const remaining =
                  used === undefined ? undefined : Math.max(0, Math.min(100, 100 - used));
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
                const resetLabel = reset
                  ? t("quota.resetsAt", { time: reset })
                  : t("quota.resetUnknown");
                const countdown =
                  resetSeconds === undefined
                    ? undefined
                    : t("quota.resetIn", { time: formatResetRemaining(resetSeconds, locale) });
                return (
                  <article className={`edge-quota-window band-${band}`} key={window.id}>
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
              })
            )}
            {active?.stale ? <p className="edge-note">{t("edge.stale")}</p> : null}
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
              {state.side === "left" ? <ChevronRight size={13} /> : <ChevronLeft size={13} />}
            </button>
          </footer>
        </div>
      </section>
    </main>
  );
}
