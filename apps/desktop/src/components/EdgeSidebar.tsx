import { listen } from "@tauri-apps/api/event";
import { Activity, ArrowUpRight, ChevronLeft, ChevronRight, Pin, Settings2, X } from "lucide-react";
import { useEffect, useRef, useState, type CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../lib/api";
import { agentName } from "../lib/format";
import { useLiveSnapshot } from "../lib/useLiveSnapshot";
import type { EdgeState, LiveSession, Locale } from "../types";
import { AgentIcon } from "./AgentIcon";
import { formatLiveElapsed, liveElapsedEnd, notchPulseValue } from "./NotchSurface";

export const EDGE_FOLD_GRACE_MS = 450;
const initialState: EdgeState = { enabled: false, expanded: false, pinned: false, side: "right" };

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
  const cancelFold = () => { clearTimeout(foldTimer.current); };
  const updateState = (next: EdgeState) => { stateRef.current = next; setState(next); };
  useEffect(() => {
    let disposed = false;
    let cleanup: (() => void) | undefined;
    // Subscribe before the initial read; state events must not be lost at mount.
    void listen<EdgeState>("edge-state", ({ payload }) => { if (!disposed) updateState(payload); })
      .then(async (unlisten) => {
        if (disposed) { unlisten(); return; }
        cleanup = unlisten;
        const current = await api.edgeState();
        if (!disposed) updateState(current);
      }).catch(() => { if (!disposed) setCommandError(true); });
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => { disposed = true; cleanup?.(); clearInterval(timer); clearTimeout(foldTimer.current); };
  }, []);
  const expand = async (expanded: boolean, pinned?: boolean) => {
    cancelFold();
    setCommandError(false);
    try { await api.setEdgeExpanded(expanded, pinned); }
    catch { setCommandError(true); }
  };
  const scheduleFold = () => {
    cancelFold();
    if (stateRef.current.pinned) return;
    foldTimer.current = setTimeout(() => {
      if (!stateRef.current.pinned && !(panelRef.current?.contains(document.activeElement) && document.activeElement?.matches(":focus-visible"))) void expand(false);
    }, EDGE_FOLD_GRACE_MS);
  };
  const sessions = snapshot.data?.sessions ?? [];
  const providers = [...new Set(sessions.map((session) => session.agent))];
  const selectedProvider = provider && providers.includes(provider) ? provider : undefined;
  const visible = sessions.filter((session) => !selectedProvider || session.agent === selectedProvider);
  const urgent = snapshot.data?.attentionAvailable && snapshot.data.attentionQueue.some((item) => item.kind !== "completion-review");
  const jump = async (session: LiveSession) => {
    if (pending) return;
    setPending(session.id); setJumpError(undefined);
    try { await api.jumpToLiveSession(session.id); }
    catch { setJumpError(session.id); }
    finally { setPending(undefined); }
  };
  const openMain = async (settings = false) => {
    try { if (settings) await api.showSettings(); else await api.showMain(); }
    catch { setCommandError(true); }
  };
  return (
    <main ref={panelRef} className={`edge-surface side-${state.side} ${state.expanded ? "is-expanded" : "is-folded"}`}
      aria-label={t("edge.title")} onPointerEnter={() => { cancelFold(); if (!stateRef.current.expanded) void expand(true); }}
      onPointerLeave={scheduleFold} onFocus={cancelFold} onBlur={scheduleFold}
      onKeyDown={(event) => { if (event.key === "Escape") { event.preventDefault(); (document.activeElement as HTMLElement)?.blur(); void expand(false, false); } }}>
      <button className={`edge-handle ${urgent ? "needs-attention" : ""}`} aria-label={t("edge.open")} aria-expanded={state.expanded}
        tabIndex={state.expanded ? -1 : 0} onClick={() => void expand(true)}><span /><span /><span /></button>
      <div className="edge-sheet" inert={!state.expanded} aria-hidden={!state.expanded}>
        <nav className="edge-rail" aria-label={t("edge.all")}>
          <button className={!selectedProvider ? "selected" : ""} title={t("edge.all")} aria-label={t("edge.all")} aria-pressed={!selectedProvider} onClick={() => setProvider(undefined)}><Activity size={20} /></button>
          <div className="edge-providers">{providers.map((agent, index) => (
            <button key={agent} style={{ "--edge-index": index } as CSSProperties} className={selectedProvider === agent ? "selected" : ""}
              title={agentName(agent)} aria-label={agentName(agent)} aria-pressed={selectedProvider === agent}
              onPointerEnter={() => setProvider(agent)} onFocus={() => setProvider(agent)} onClick={() => setProvider(agent)}>
              <AgentIcon agent={agent} size={22} /><small>{sessions.filter((s) => s.agent === agent).length}</small>
            </button>
          ))}</div>
          <button className="edge-settings" title={t("edge.settings")} aria-label={t("edge.settings")} onClick={() => void openMain(true)}><Settings2 size={17} /></button>
        </nav>
        <section className="edge-detail">
          <header><div><small>VIBEMETER</small><h1>{selectedProvider ? agentName(selectedProvider) : t("edge.recent")}</h1></div>
            <button aria-label={t(state.pinned ? "edge.unpin" : "edge.pin")} aria-pressed={state.pinned} className={state.pinned ? "selected" : ""} onClick={() => void expand(true, !state.pinned)}><Pin size={15} /></button>
            <button aria-label={t("edge.close")} onClick={() => { (document.activeElement as HTMLElement)?.blur(); void expand(false, false); }}><X size={15} /></button>
          </header>
          <div className="edge-session-list" key={selectedProvider ?? "all"}>
            {snapshot.isLoading ? <p className="edge-empty" role="status">{t("edge.loading")}</p> : snapshot.isError ? <div className="edge-empty" role="alert"><strong>{t("edge.error")}</strong><button onClick={() => void snapshot.refetch()}>{t("edge.retry")}</button></div> : !visible.length ? <div className="edge-empty"><Activity size={28} /><strong>{t("edge.empty")}</strong><p>{t("edge.emptyBody")}</p></div> : visible.map((session, index) => {
              const exact = session.pulse.lifecycle.availability === "available";
              const lifecycle = exact ? session.pulse.lifecycle.value ?? "idle" : "unknown";
              const pulse = notchPulseValue(session);
              return <article key={session.id} className={`edge-session status-${lifecycle}`} style={{ "--edge-index": index } as CSSProperties}>
                <div className="edge-session-heading"><AgentIcon agent={session.agent} size={16} /><strong title={session.projectLabel}>{session.projectLabel}</strong><span className="edge-status-dot" /></div>
                <h2>{session.conversationTitle || session.projectLabel}</h2>
                <div className="edge-session-meta"><span>{exact ? t(`live.status.${lifecycle}`, { defaultValue: t("edge.unavailable") }) : t("edge.unavailable")}</span><time>{formatLiveElapsed(session.startedAt, liveElapsedEnd(session, now))}</time></div>
                <p className="edge-phase">{t(`notch.phase.${pulse}`, { defaultValue: t(`live.pulse.value.${pulse}`, { defaultValue: t("edge.unavailable") }) })}</p>
                <button className="edge-jump" disabled={Boolean(pending)} onClick={() => void jump(session)}>{t("edge.jump")}<ArrowUpRight size={13} /></button>
                {jumpError === session.id ? <p className="edge-error" role="alert">{t("edge.jumpError")}</p> : null}
              </article>;
            })}
          </div>
          {commandError ? <p className="edge-error" role="alert">{t("edge.error")}</p> : null}
          <footer><span>{t("edge.count", { count: visible.length })}</span><button onClick={() => void openMain()}>{t("edge.overview")}{state.side === "left" ? <ChevronRight size={13} /> : <ChevronLeft size={13} />}</button></footer>
        </section>
      </div>
    </main>
  );
}
