import { useQuery } from "@tanstack/react-query";
import { ArrowRight, CalendarRange, DatabaseZap, Eye, EyeOff, Fingerprint, ScanFace, ShieldCheck, Sparkles, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { VctiAvatar } from "../components/VctiAvatar";
import { EmptyState, ErrorState, LoadingState } from "../components/ui";
import { api } from "../lib/api";
import { formatCompact, formatDate, formatDuration, formatPercent } from "../lib/format";
import { VCTI_GUILDS, VCTI_TYPES } from "../lib/vctiCatalog";
import { useUiStore } from "../store";
import type { Locale, VctiEvidenceItem, VctiProfile } from "../types";

function evidenceValue(item: VctiEvidenceItem, locale: Locale): string {
  if (item.format === "percent") return formatPercent(item.value / 100, locale);
  if (item.format === "duration") return formatDuration(Math.round(item.value), locale);
  return formatCompact(item.value, locale);
}

function BehaviorCoverage({ profile }: { profile: VctiProfile }) {
  const { t } = useTranslation();
  const streams = [
    ["structure", profile.behavior.structureCoverage, profile.behavior.structureCapableSessions],
    ["lifecycle", profile.behavior.lifecycleCoverage, profile.behavior.lifecycleCapableSessions],
    ["orchestration", profile.behavior.orchestrationCoverage, profile.behavior.orchestrationCapableSessions],
    ["toolResults", profile.behavior.toolResultCoverage, profile.behavior.toolResultCapableSessions],
    ["processControl", profile.behavior.processControlCoverage, profile.behavior.processControlCapableSessions],
  ] as const;
  const total = profile.behavior.sessions;
  return (
    <div className="vcti-coverage-list">
      {streams.map(([id, value, observed]) => (
        <div key={id} className={value < .5 ? "limited" : ""}>
          <span><b>{t(`behavior.${id}.title`)}</b><small>{observed} / {total} · {Math.round(value * 100)}%</small></span>
          <i><em style={{ width: `${Math.round(value * 100)}%` }} /></i>
          <p>{value < .5 ? t("vcti.coverageLimited", { observed, total }) : t("vcti.coverageObserved", { observed, total })}</p>
        </div>
      ))}
    </div>
  );
}

export function VctiPage({ locale }: { locale: Locale }) {
  const { t } = useTranslation();
  const [showAtlas, setShowAtlas] = useState(false);
  const openShare = useUiStore((state) => state.openShare);
  const query = useQuery({ queryKey: ["vcti"], queryFn: api.vctiProfile, refetchInterval: 60_000 });
  const scoreMap = useMemo(
    () => Object.fromEntries(query.data?.scores.map((score) => [score.id, score]) ?? []),
    [query.data?.scores],
  );
  useEffect(() => {
    if (!showAtlas) return;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setShowAtlas(false);
    };
    document.body.classList.add("modal-open");
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      document.body.classList.remove("modal-open");
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, [showAtlas]);
  if (query.isLoading) return <LoadingState />;
  if (query.isError || !query.data) return <ErrorState retry={() => void query.refetch()} />;
  const profile = query.data;
  const primaryName = profile.primaryType ? t(`vcti.types.${profile.primaryType}.name`) : t("vcti.collecting.name");
  const primaryTagline = profile.primaryType ? t(`vcti.types.${profile.primaryType}.tagline`) : t("vcti.collecting.body");
  const guildName = profile.guild ? t(`vcti.guilds.${profile.guild}.name`) : t("vcti.collecting.guild");
  const stable = profile.status === "stable" || profile.status === "high-confidence";

  return (
    <div className="page vcti-page">
      <header className="vcti-page-header">
        <div>
          <span className="eyebrow"><Fingerprint size={13} />{t("vcti.eyebrow")}</span>
          <h1>{t("vcti.title")}</h1>
          <p>{t("vcti.description")}</p>
        </div>
        <div className="vcti-period"><CalendarRange size={15} /><span>{t("vcti.canonicalWindow")}</span><strong>{formatDate(profile.periodStart, locale, "short")} — {formatDate(profile.periodEnd, locale, "short")}</strong></div>
      </header>

      <section className={`vcti-reveal ${stable ? "stable" : profile.status}`}>
        <div className="vcti-reveal-copy">
          <span className="vcti-guild-label">{guildName}</span>
          <h2>{profile.primaryType ? <b>{profile.primaryType}</b> : null}{primaryName}</h2>
          <p>{primaryTagline}</p>
          <div className="vcti-meta-row">
            <span><DatabaseZap size={14} />{t("vcti.observed", { sessions: profile.sessionCount, days: profile.activeDays })}</span>
            <span><ShieldCheck size={14} />{t(`vcti.confidence.${profile.confidenceLabel}`)} · {Math.round(profile.confidence)}%</span>
          </div>
          {profile.primaryType ? (
            <div className="vcti-identity-strip">
              {profile.secondaryType ? <span>{t("vcti.secondary")} <strong>{profile.secondaryType} · {t(`vcti.types.${profile.secondaryType}.name`)}</strong></span> : null}
              {profile.badges.map((badge) => <span className="vcti-badge" key={badge.code}>{badge.code} · {t(badge.labelKey)}</span>)}
            </div>
          ) : (
            <div className="vcti-collecting-track"><span style={{ width: `${Math.min(92, profile.confidence)}%` }} /></div>
          )}
          <div className="vcti-actions">
            <button className="button primary" disabled={!profile.primaryType} onClick={() => openShare("vcti-card")}><Sparkles size={14} />{t("vcti.makeShareCard")}</button>
            <button className="button subtle" onClick={() => setShowAtlas((value) => !value)}>{showAtlas ? <EyeOff size={14} /> : <Eye size={14} />}{showAtlas ? t("vcti.hideAtlas") : t("vcti.showAtlas")}</button>
          </div>
        </div>
        <VctiAvatar type={profile.primaryType} guild={profile.guild} label={primaryName} />
      </section>

      <div className="vcti-dashboard-grid">
        <section className="vcti-panel vcti-scores">
          <header><span className="section-index">01</span><div><h2>{t("vcti.scoresTitle")}</h2><p>{t("vcti.scoresBody")}</p></div></header>
          <div className="vcti-score-list">
            {["startStructure", "delegation", "guardrail", "debugDepth", "shipping", "toolNomad"].map((id) => {
              const score = scoreMap[id];
              return (
                <div key={id} className={score && score.coverage < .5 ? "low-coverage" : ""}>
                  <span><b>{t(`vcti.scores.${id}`)}</b><strong>{score ? Math.round(score.value) : "—"}</strong></span>
                  <i><em style={{ width: `${score?.value ?? 0}%` }} /></i>
                  {score && score.coverage < .5 ? <small>{t("vcti.partialEvidence")}</small> : null}
                </div>
              );
            })}
          </div>
        </section>

        <section className="vcti-panel vcti-evidence">
          <header><span className="section-index">02</span><div><h2>{t("vcti.evidenceTitle")}</h2><p>{t("vcti.evidenceBody")}</p></div></header>
          {profile.evidence.length ? <div className="vcti-evidence-list">{profile.evidence.map((item) => (
            <div key={item.id}>
              <span className={item.structural ? "structural" : ""}>{item.structural ? t("vcti.structureChip") : t("vcti.observedChip")}</span>
              <strong>{evidenceValue(item, locale)}</strong>
              <p>{t(item.labelKey)}</p>
            </div>
          ))}</div> : <EmptyState title={t("vcti.noEvidence")} body={t("vcti.collecting.body")} />}
        </section>

        <section className="vcti-panel vcti-coverage">
          <header><span className="section-index">03</span><div><h2>{t("vcti.coverageTitle")}</h2><p>{t("vcti.coverageBody")}</p></div></header>
          <BehaviorCoverage profile={profile} />
          {profile.missingCapabilities.length ? <div className="vcti-capability-note"><strong>{t("vcti.missingCapability", { count: profile.missingCapabilities.length })}</strong><span>{profile.missingCapabilities.map((capability) => t(`vcti.capabilityNames.${capability}`)).join(" · ")}</span></div> : <p className="vcti-capability-note complete">{t("vcti.coverageComplete")}</p>}
        </section>

        <section className="vcti-panel vcti-trend">
          <header><span className="section-index">04</span><div><h2>{t("vcti.trendTitle")}</h2><p>{t("vcti.trendBody")}</p></div></header>
          <div className="vcti-trend-list">
            {profile.trend.length ? profile.trend.map((point) => (
              <div key={point.periodStart}>
                <small>{formatDate(point.periodStart, locale, "short")}</small>
                <span>{point.dominantType ? <><b>{point.dominantType}</b>{t(`vcti.types.${point.dominantType}.name`)}</> : t("vcti.collecting.name")}</span>
                <i><em style={{ width: `${point.scores.find((score) => score.id === "shipping")?.value ?? 0}%` }} /></i>
              </div>
            )) : <p>{t("vcti.trendUnavailable")}</p>}
          </div>
        </section>
      </div>

      {showAtlas ? createPortal(
        <div className="modal-backdrop vcti-atlas-backdrop" role="presentation" onMouseDown={(event) => {
          if (event.target === event.currentTarget) setShowAtlas(false);
        }}>
          <section className="vcti-atlas-window" role="dialog" aria-modal="true" aria-labelledby="vcti-atlas-title">
            <header>
              <div>
                <span className="eyebrow"><ScanFace size={13} />VCTI / 24</span>
                <h2 id="vcti-atlas-title">{t("vcti.atlasTitle")}</h2>
                <p>{t("vcti.atlasBody")}</p>
              </div>
              <button className="icon-button" onClick={() => setShowAtlas(false)} aria-label={t("actions.close")}><X size={18} /></button>
            </header>
            <div className="vcti-atlas-window-body">
              {VCTI_GUILDS.map((guild, guildIndex) => (
                <section className="vcti-atlas-guild" key={guild}>
                  <header>
                    <span className="section-index">{String(guildIndex + 1).padStart(2, "0")}</span>
                    <div><strong>{t(`vcti.guilds.${guild}.name`)}</strong><small>{t(`vcti.guilds.${guild}.description`)}</small></div>
                  </header>
                  <div className="vcti-atlas-types">
                    {VCTI_TYPES.filter(([, typeGuild]) => typeGuild === guild).map(([code]) => (
                      <article className={profile.primaryType === code ? "active" : ""} key={code}>
                        <VctiAvatar type={code} size="small" label={t(`vcti.types.${code}.name`)} />
                        <div><b>{code}</b><strong>{t(`vcti.types.${code}.name`)}</strong><p>{t(`vcti.types.${code}.tagline`)}</p></div>
                      </article>
                    ))}
                  </div>
                </section>
              ))}
            </div>
            <footer>
              <span>{profile.primaryType ? `${profile.primaryType} · ${primaryName}` : t("vcti.collecting.name")}</span>
              <button className="button primary" disabled={!profile.primaryType} onClick={() => {
                setShowAtlas(false);
                openShare("vcti-card");
              }}>{t("vcti.makeShareCard")}<ArrowRight size={14} /></button>
            </footer>
          </section>
        </div>,
        document.body,
      ) : null}
    </div>
  );
}
