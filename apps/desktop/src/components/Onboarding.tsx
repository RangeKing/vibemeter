import { useQuery } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import {
  DatabaseZap,
  Fingerprint,
  GitBranch,
  HardDrive,
  RadioTower,
  ScanFace,
  ScanSearch,
  ShieldCheck,
  Sparkles,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import appIconUrl from "../../src-tauri/icons/vibemeter-icon-source.png";
import { api } from "../lib/api";
import { agentName, formatCompact } from "../lib/format";
import { sourceCapabilityNameGroups } from "../lib/sourceStatus";
import type { IndexStatus, Locale, VctiProfile } from "../types";
import { AgentBadge, Toggle } from "./ui";
import { VctiAvatar } from "./VctiAvatar";

type Step = "consent" | "scan" | "reveal";

const PREVIEW_SESSION_TARGET = 8;

function progressPercent(status?: IndexStatus): number {
  if (!status) return 0;
  if (!status.running && status.finishedAt) return 100;
  if (status.discoveredFiles <= 0) return status.running ? 8 : 0;
  return Math.min(99, Math.round((status.processedFiles / status.discoveredFiles) * 100));
}

function ScanFeed({ status, locale }: { status?: IndexStatus; locale: Locale }) {
  const { t } = useTranslation();
  const sources = useQuery({
    queryKey: ["sources"],
    queryFn: api.sources,
    refetchInterval: status?.running ? 1_200 : false,
  });
  const available = (sources.data ?? []).filter((source) => source.available);
  const lines = [
    t(status?.messageKey ?? "index.idle"),
    status
      ? t("onboarding.scanFiles", {
          processed: formatCompact(status.processedFiles, locale),
          total: formatCompact(status.discoveredFiles, locale),
        })
      : t("onboarding.scanStarting"),
    status
      ? t("onboarding.scanSessions", { count: formatCompact(status.indexedSessions, locale) })
      : null,
    available.length
      ? t("onboarding.scanAgents", {
          agents: available.map((source) => agentName(source.agent)).join(" · "),
        })
      : t("onboarding.scanAgentsPending"),
  ].filter(Boolean) as string[];

  return (
    <div className="onboarding-scan">
      <div className="onboarding-scan-meter" aria-hidden="true">
        <span style={{ width: `${progressPercent(status)}%` }} />
      </div>
      <div className="onboarding-scan-stats">
        <div><strong>{formatCompact(status?.discoveredFiles ?? 0, locale)}</strong><span>{t("onboarding.statFiles")}</span></div>
        <div><strong>{formatCompact(status?.indexedSessions ?? 0, locale)}</strong><span>{t("onboarding.statSessions")}</span></div>
        <div><strong>{formatCompact(available.length, locale)}</strong><span>{t("onboarding.statAgents")}</span></div>
      </div>
      <div className="onboarding-scan-feed" aria-live="polite">
        {lines.map((line, index) => (
          <p key={`${line}-${index}`} className={index === lines.length - 1 ? "active" : undefined}>{line}</p>
        ))}
      </div>
      {available.length ? (
        <div className="onboarding-agent-strip">
          {available.map((source) => (
            <span key={source.agent}>
              <AgentBadge agent={source.agent} compact />
              <small>{formatCompact(source.sessionCount, locale)}</small>
            </span>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function RevealCard({ profile }: { profile?: VctiProfile; locale: Locale }) {
  const { t } = useTranslation();
  if (!profile) {
    return (
      <div className="onboarding-reveal collecting">
        <div className="onboarding-reveal-copy">
          <span className="eyebrow"><Fingerprint size={13} />{t("onboarding.revealEyebrow")}</span>
          <h2>{t("onboarding.revealLoading")}</h2>
          <p>{t("onboarding.revealLoadingBody")}</p>
        </div>
      </div>
    );
  }
  const ready = Boolean(profile.primaryType) && profile.status !== "collecting";
  const remaining = Math.max(0, PREVIEW_SESSION_TARGET - profile.sessionCount);
  const primaryName = profile.primaryType ? t(`vcti.types.${profile.primaryType}.name`) : t("vcti.collecting.name");
  const primaryTagline = profile.primaryType ? t(`vcti.types.${profile.primaryType}.tagline`) : t("vcti.collecting.body");
  const guildName = profile.guild ? t(`vcti.guilds.${profile.guild}.name`) : t("vcti.collecting.guild");
  return (
    <div className={`onboarding-reveal ${ready ? "ready" : "collecting"}`}>
      <div className="onboarding-reveal-copy">
        <span className="eyebrow"><Fingerprint size={13} />{t("onboarding.revealEyebrow")}</span>
        <span className="onboarding-guild">{guildName}</span>
        <h2>{profile.primaryType ? <><b>{profile.primaryType}</b>{primaryName}</> : primaryName}</h2>
        <p>{ready ? primaryTagline : remaining > 0 ? t("onboarding.revealNeedMore", { count: remaining }) : t("vcti.collecting.body")}</p>
        <div className="onboarding-reveal-meta">
          <span><DatabaseZap size={14} />{t("vcti.observed", { sessions: profile.sessionCount, days: profile.activeDays })}</span>
          <span><ShieldCheck size={14} />{t(`vcti.confidence.${profile.confidenceLabel}`)}</span>
        </div>
        {!ready ? (
          <div className="vcti-collecting-track">
            <span style={{ width: `${Math.min(92, (profile.sessionCount / PREVIEW_SESSION_TARGET) * 100)}%` }} />
          </div>
        ) : null}
      </div>
      <VctiAvatar type={profile.primaryType} guild={profile.guild} label={primaryName} />
      <p className="onboarding-reveal-note">{t("onboarding.revealNote")}</p>
    </div>
  );
}

export function Onboarding({ onComplete }: { onComplete: () => Promise<void> }) {
  const { t, i18n } = useTranslation();
  const locale: Locale = i18n.language.toLowerCase().startsWith("zh") ? "zh-CN" : "en-US";
  const capabilityNames = sourceCapabilityNameGroups(locale === "zh-CN" ? "、" : ", ");
  const [step, setStep] = useState<Step>("consent");
  const [credentialsAllowed, setCredentialsAllowed] = useState(false);
  const [gitReadAllowed, setGitReadAllowed] = useState(false);
  const [vctiPromptStructure, setVctiPromptStructure] = useState(true);
  const [busy, setBusy] = useState(false);
  const [indexStatus, setIndexStatus] = useState<IndexStatus>();
  const [profile, setProfile] = useState<VctiProfile>();
  const scanStarted = useRef(false);
  const revealReady = useRef(false);

  useEffect(() => {
    if (step !== "scan") return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const pull = async () => {
      const status = await api.indexStatus();
      if (!disposed) setIndexStatus(status);
      return status;
    };
    void pull();
    const timer = window.setInterval(() => { void pull(); }, 900);
    void listen<IndexStatus>("index-progress", (event) => {
      if (!disposed) setIndexStatus(event.payload);
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    });
    return () => {
      disposed = true;
      window.clearInterval(timer);
      unlisten?.();
    };
  }, [step]);

  useEffect(() => {
    if (step !== "scan" || !indexStatus || revealReady.current) return;
    const done = !indexStatus.running && Boolean(indexStatus.finishedAt || indexStatus.phase === "complete" || indexStatus.phase === "partial");
    if (!done && !(indexStatus.processedFiles > 0 && indexStatus.discoveredFiles > 0 && indexStatus.processedFiles >= indexStatus.discoveredFiles && !indexStatus.running)) {
      return;
    }
    revealReady.current = true;
    const handle = window.setTimeout(() => {
      void (async () => {
        try {
          const next = await api.vctiProfile("90d");
          setProfile(next);
        } catch {
          setProfile(undefined);
        }
        setStep("reveal");
      })();
    }, 650);
    return () => window.clearTimeout(handle);
  }, [indexStatus, step]);

  const startScan = async () => {
    if (busy || scanStarted.current) return;
    setBusy(true);
    try {
      scanStarted.current = true;
      await Promise.all([
        api.setSetting("credentialsAllowed", String(credentialsAllowed)),
        api.setSetting("gitReadAllowed", String(gitReadAllowed)),
        api.setSetting("vctiPromptStructure", String(vctiPromptStructure)),
        api.setSetting("liveHooksEnabled", "true"),
        api.setSetting("notchEnabled", "true"),
        api.setSetting("menuBarEnabled", "true"),
        api.setSetting("iaMigrationTipSeen", "true"),
      ]);
      if (credentialsAllowed) await api.refreshProviders(true, false);
      setStep("scan");
      await api.refreshIndex(true);
      setIndexStatus(await api.indexStatus());
    } catch {
      scanStarted.current = false;
      setStep("consent");
    } finally {
      setBusy(false);
    }
  };

  const finish = async () => {
    if (busy) return;
    setBusy(true);
    try {
      await api.setSetting("onboardingComplete", "true");
      await onComplete();
    } finally {
      setBusy(false);
    }
  };

  const features = [
    [HardDrive, t("onboarding.localTitle"), t("onboarding.localBody")],
    [RadioTower, t("onboarding.liveTitle"), t("onboarding.liveBody", capabilityNames)],
    [ScanFace, t("onboarding.vctiTitle"), t("onboarding.vctiBody")],
    [ShieldCheck, t("onboarding.shareTitle"), t("onboarding.shareBody")],
  ] as const;

  return (
    <div className={`onboarding-window step-${step}`}>
      <div className="titlebar-drag" data-tauri-drag-region />
      <section className={`onboarding-card ${step}`}>
        <div className="onboarding-brand"><img src={appIconUrl} alt="" /><span>{t("app.name")}</span></div>
        <div className="onboarding-steps" aria-hidden="true">
          {(["consent", "scan", "reveal"] as Step[]).map((item, index) => (
            <span key={item} className={step === item ? "active" : (["consent", "scan", "reveal"].indexOf(step) > index ? "done" : "")}>
              {String(index + 1).padStart(2, "0")}
            </span>
          ))}
        </div>

        {step === "consent" ? (
          <>
            <span className="eyebrow">{t("onboarding.eyebrow")}</span>
            <h1>{t("onboarding.title")}</h1>
            <p className="lead">{t("onboarding.body")}</p>
            <div className="onboarding-list">
              {features.map(([Icon, title, body]) => (
                <div className="onboarding-item" key={title}>
                  <span><Icon size={18} /></span>
                  <div><strong>{title}</strong><p>{body}</p></div>
                </div>
              ))}
            </div>
            <div className="consent-stack">
              <label className="consent-row">
                <span><strong>{t("onboarding.vctiStructure")}</strong><small><ScanSearch size={12} />{t("onboarding.vctiStructureBody")}</small></span>
                <Toggle checked={vctiPromptStructure} onCheckedChange={setVctiPromptStructure} label={t("onboarding.vctiStructure")} />
              </label>
              <label className="consent-row">
                <span><strong>{t("onboarding.allowGit")}</strong><small><GitBranch size={12} />{t("onboarding.gitOff")}</small></span>
                <Toggle checked={gitReadAllowed} onCheckedChange={setGitReadAllowed} label={t("onboarding.allowGit")} />
              </label>
              <label className="consent-row">
                <span><strong>{t("onboarding.allowCredentials")}</strong></span>
                <Toggle checked={credentialsAllowed} onCheckedChange={setCredentialsAllowed} label={t("onboarding.allowCredentials")} />
              </label>
            </div>
            <button className="button primary full" onClick={() => void startScan()} disabled={busy}>
              <Sparkles size={15} />{busy ? t("actions.refreshing") : t("onboarding.startScan")}
            </button>
          </>
        ) : null}

        {step === "scan" ? (
          <>
            <span className="eyebrow">{t("onboarding.scanEyebrow")}</span>
            <h1>{t("onboarding.scanTitle")}</h1>
            <p className="lead">{t("onboarding.scanBody")}</p>
            <ScanFeed status={indexStatus} locale={locale} />
            <button
              className="button subtle full"
              disabled={busy || Boolean(indexStatus?.running)}
              onClick={() => {
                revealReady.current = true;
                void api.vctiProfile("90d").then(setProfile).catch(() => setProfile(undefined)).finally(() => setStep("reveal"));
              }}
            >
              {t("onboarding.skipWait")}
            </button>
          </>
        ) : null}

        {step === "reveal" ? (
          <>
            <span className="eyebrow">{t("onboarding.revealStepEyebrow")}</span>
            <h1>{t("onboarding.revealTitle")}</h1>
            <p className="lead">{t("onboarding.revealBody")}</p>
            <RevealCard profile={profile} locale={locale} />
            <button className="button primary full" onClick={() => void finish()} disabled={busy}>
              {busy ? t("actions.refreshing") : t("onboarding.finish")}
            </button>
          </>
        ) : null}
      </section>
    </div>
  );
}
