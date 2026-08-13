import { useQuery } from "@tanstack/react-query";
import { ArrowRight, CalendarRange, DatabaseZap, Eye, EyeOff, Orbit, ScanFace, ShieldCheck, Sparkles, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { BehaviorStreams } from "../components/BehaviorStreams";
import { CatchphraseClouds } from "../components/CatchphraseClouds";
import { InsightCard } from "../components/InsightCard";
import { RangePicker } from "../components/RangePicker";
import { VctiAvatar } from "../components/VctiAvatar";
import { VctiArtPortrait } from "../components/VctiArtPortrait";
import { EmptyState, ErrorState, LoadingState } from "../components/ui";
import { api } from "../lib/api";
import { formatCompact, formatDate, formatDuration, formatPercent } from "../lib/format";
import { useFocusTrap } from "../lib/useFocusTrap";
import { VCTI_BADGES, VCTI_GUILDS, VCTI_TYPES } from "../lib/vctiCatalog";
import { useUiStore } from "../store";
import type { Locale, VctiEvidenceItem, VctiOptionalMetric, VctiProfile } from "../types";

function evidenceValue(item: VctiEvidenceItem, locale: Locale): string {
  if (item.format === "percent") return formatPercent(item.value / 100, locale);
  if (item.format === "duration") return formatDuration(Math.round(item.value), locale);
  return formatCompact(item.value, locale);
}

function IdentityEvidenceSummary({ profile }: { profile: VctiProfile }) {
  const { t } = useTranslation();
  const summaries = [
    {
      id: "rhythm",
      label: t("vcti.identityEvidence.rhythm"),
      value: t("vcti.visualLegend.rhythm"),
      available: profile.identityVisual.rhythm.available,
    },
    {
      id: "collaboration",
      label: t("vcti.identityEvidence.collaboration"),
      value: t("vcti.visualLegend.collaboration"),
      available: profile.identityVisual.collaboration.available,
    },
    {
      id: "detail",
      label: t("vcti.identityEvidence.detail"),
      value: t("vcti.visualLegend.detail"),
      available: profile.identityVisual.detail.available,
    },
    {
      id: "process",
      label: t("vcti.identityEvidence.process"),
      value: t("vcti.visualLegend.process"),
      available: profile.identityVisual.process.available,
    },
  ];

  return (
    <ul className="vcti-visual-legend" aria-label={t("vcti.identityEvidence.title")}>
      {summaries.map((item) => (
        <li key={item.id} className={item.available ? "mapped" : "not-recorded"}>
          <i aria-hidden="true" />
          <b>{item.label}</b>
          <span>{item.available ? item.value : t("vcti.shortNotRecorded")}</span>
        </li>
      ))}
    </ul>
  );
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
  const range = useUiStore((state) => state.range);
  const setRange = useUiStore((state) => state.setRange);
  const openShare = useUiStore((state) => state.openShare);
  const openSessions = useUiStore((state) => state.openSessions);
  const [showAtlas, setShowAtlas] = useState(false);
  const [showEvidence, setShowEvidence] = useState(false);
  const evidenceTrapRef = useFocusTrap(showEvidence);
  const atlasTrapRef = useFocusTrap(showAtlas);
  const query = useQuery({
    queryKey: ["vcti", range],
    queryFn: () => api.vctiProfile(range),
    refetchInterval: 60_000,
  });
  const insights = useQuery({
    queryKey: ["insights", range],
    queryFn: () => api.insights(range),
  });
  const phrases = useQuery({
    queryKey: ["phrase-cloud", range],
    queryFn: () => api.phraseCloud(range),
    refetchInterval: 30_000,
  });
  const scoreMap = useMemo(
    () => Object.fromEntries(query.data?.scores.map((score) => [score.id, score]) ?? []),
    [query.data?.scores],
  );
  useEffect(() => {
    if (!showAtlas && !showEvidence) return;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      setShowAtlas(false);
      setShowEvidence(false);
    };
    document.body.classList.add("modal-open");
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      document.body.classList.remove("modal-open");
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, [showAtlas, showEvidence]);
  if (query.isLoading) return <LoadingState />;
  if (query.isError || !query.data) return <ErrorState retry={() => void query.refetch()} />;
  const profile = query.data;
  const primaryName = profile.primaryType ? t(`vcti.types.${profile.primaryType}.name`) : t("vcti.collecting.name");
  const primaryTagline = profile.primaryType ? t(`vcti.types.${profile.primaryType}.tagline`) : t("vcti.collecting.body");
  const guildName = profile.guild ? t(`vcti.guilds.${profile.guild}.name`) : t("vcti.collecting.guild");
  const stable = !profile.temporary && (profile.status === "stable" || profile.status === "high-confidence");
  const showRangeNudge = range !== "90d";
  const earnedBadges = new Set(profile.badges.map((badge) => badge.code));

  return (
    <div className="page vcti-page">
      <header className="vcti-page-header">
        <div>
          <span className="eyebrow"><Orbit size={13} />{t("vcti.eyebrow")}</span>
          <h1>{t("vcti.title")}</h1>
          <p>{t("vcti.description")}</p>
        </div>
        <div className="vcti-header-actions">
          <RangePicker />
          <div className="vcti-period">
            <CalendarRange size={15} />
            <span>{profile.temporary ? t("vcti.temporaryWindow") : t("vcti.canonicalWindow")}</span>
            <strong>{formatDate(profile.periodStart, locale, "short")} — {formatDate(profile.periodEnd, locale, "short")}</strong>
          </div>
        </div>
      </header>

      {showRangeNudge ? (
        <aside className="vcti-range-nudge" role="status">
          <div>
            <strong>{t("vcti.temporaryBadge")}</strong>
            <p>{t("vcti.suggest90d")}</p>
          </div>
          <button className="button subtle" onClick={() => setRange("90d")}>{t("vcti.use90d")}</button>
        </aside>
      ) : null}

      <section className={`vcti-reveal ${stable ? "stable" : profile.status}`}>
        <div className="vcti-reveal-copy">
          <span className="vcti-guild-label">{guildName}</span>
          <h2>{profile.primaryType ? <b>{profile.primaryType}</b> : null}{primaryName}</h2>
          <p>{primaryTagline}</p>
          <div className="vcti-meta-row">
            <span><DatabaseZap size={14} />{t("vcti.observed", { sessions: profile.sessionCount, days: profile.activeDays })}</span>
            <button className="vcti-evidence-trigger" type="button" onClick={() => setShowEvidence(true)}>
              <ShieldCheck size={14} />
              {profile.temporary ? t("vcti.temporaryBadge") : t(`vcti.confidence.${profile.confidenceLabel}`)}
              · {Math.round(profile.confidence)}%
              <small>{t("vcti.openEvidence")}</small>
            </button>
          </div>
          {profile.primaryType ? (
            <div className="vcti-identity-strip">
              {profile.secondaryType ? <span>{t("vcti.secondary")} <strong>{profile.secondaryType} · {t(`vcti.types.${profile.secondaryType}.name`)}</strong></span> : null}
              {profile.badges.map((badge) => (
                <span className="vcti-badge" key={badge.code} title={t(badge.descriptionKey)}>
                  {badge.code} · {t(badge.labelKey)}
                </span>
              ))}
            </div>
          ) : (
            <div className="vcti-collecting-track"><span style={{ width: `${Math.min(92, profile.confidence)}%` }} /></div>
          )}
          {profile.primaryType ? <IdentityEvidenceSummary profile={profile} /> : null}
          <div className="vcti-actions">
            <button className="button primary" disabled={!profile.primaryType} onClick={() => openShare("vcti-card")}><Sparkles size={14} />{t("vcti.makeShareCard")}</button>
            <button className="button subtle" onClick={() => setShowAtlas((value) => !value)}>{showAtlas ? <EyeOff size={14} /> : <Eye size={14} />}{showAtlas ? t("vcti.hideAtlas") : t("vcti.showAtlas")}</button>
          </div>
        </div>
        <VctiArtPortrait visual={profile.identityVisual} type={profile.primaryType} guild={profile.guild} label={primaryName} />
      </section>

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

      {phrases.data ? <CatchphraseClouds data={phrases.data} locale={locale} /> : phrases.isError ? (
        <section className="catchphrase-section">
          <ErrorState retry={() => void phrases.refetch()} />
        </section>
      ) : null}

      <section className="vcti-panel vcti-insights">
        <header><span className="section-index">02</span><div><h2>{t("insights.title")}</h2><p>{t("insights.description")}</p></div></header>
        {insights.isLoading ? <LoadingState /> : insights.isError || !insights.data ? (
          <ErrorState retry={() => void insights.refetch()} />
        ) : insights.data.items.length ? (
          <div className="insight-grid">
            {insights.data.items.map((item, index) => (
              <InsightCard
                key={item.id}
                item={item}
                index={index}
                locale={locale}
                onOpenSession={(sessionId) => {
                  openSessions(sessionId);
                }}
              />
            ))}
          </div>
        ) : (
          <EmptyState title={t("insights.noItems")} body={t("insights.description")} />
        )}
      </section>

      <section className="vcti-panel vcti-behavior">
        <header><span className="section-index">03</span><div><h2>{t("behavior.insightTitle")}</h2><p>{t("behavior.insightBody")}</p></div></header>
        <BehaviorStreams data={profile.behavior} locale={locale} compact />
      </section>

      {showEvidence ? createPortal(
        <div className="modal-backdrop vcti-evidence-backdrop" role="presentation" onMouseDown={(event) => {
          if (event.target === event.currentTarget) setShowEvidence(false);
        }}>
          <section ref={evidenceTrapRef} className="vcti-evidence-drawer" role="dialog" aria-modal="true" aria-labelledby="vcti-evidence-title">
            <header>
              <div>
                <span className="eyebrow"><ShieldCheck size={13} />{t("vcti.evidenceStrength")}</span>
                <h2 id="vcti-evidence-title">{t("vcti.evidenceTitle")}</h2>
                <p>{t("vcti.evidenceBody")}</p>
              </div>
              <button className="icon-button" onClick={() => setShowEvidence(false)} aria-label={t("actions.close")}><X size={18} /></button>
            </header>
            <div className="vcti-evidence-drawer-body">
              <section>
                <h3>{t("vcti.evidenceTitle")}</h3>
                {profile.evidence.length ? (
                  <div className="vcti-evidence-list">{profile.evidence.map((item) => (
                    <div key={item.id}>
                      <span className={item.structural ? "structural" : ""}>{item.structural ? t("vcti.structureChip") : t("vcti.observedChip")}</span>
                      <strong>{evidenceValue(item, locale)}</strong>
                      <p>{t(item.labelKey)}</p>
                    </div>
                  ))}</div>
                ) : <EmptyState title={t("vcti.noEvidence")} body={t("vcti.collecting.body")} />}
              </section>
              <section>
                <h3>{t("vcti.rhythmTitle")}</h3>
                <p>{t("vcti.rhythmBody")}</p>
                <div className="vcti-rhythm-evidence">
                  <div>
                    <span>{t("vcti.workPeriods")}</span>
                    <strong>{profile.identityEvidence.rhythm.workPeriodsAvailable
                      ? profile.identityEvidence.rhythm.workPeriods
                        .filter((period) => period.sessions > 0)
                        .map((period) => `${t(`vcti.periods.${period.id}`)} ${period.sessions}`)
                        .join(" · ") || t("vcti.noActivity")
                      : t("vcti.notRecorded")}</strong>
                  </div>
                  <div>
                    <span>{t("vcti.activeDaysLabel")}</span>
                    <strong>{profile.identityEvidence.rhythm.activeDays.available && profile.identityEvidence.rhythm.activeDays.value !== undefined
                      ? t("vcti.daysValue", { value: profile.identityEvidence.rhythm.activeDays.value })
                      : t("vcti.notRecorded")}</strong>
                  </div>
                  <div>
                    <span>{t("vcti.sessionDensity")}</span>
                    <strong>{profile.identityEvidence.rhythm.sessionsPerDay.available && profile.identityEvidence.rhythm.sessionsPerDay.value !== undefined
                      ? t("vcti.sessionsPerDayValue", { value: profile.identityEvidence.rhythm.sessionsPerDay.value.toFixed(1) })
                      : t("vcti.notRecorded")}</strong>
                  </div>
                  <div>
                    <span>{t("vcti.subagentStartsLabel")}</span>
                    <strong>{profile.identityEvidence.collaboration.subagentStarts.available && profile.identityEvidence.collaboration.subagentStarts.value !== undefined
                      ? t("vcti.countValue", { value: profile.identityEvidence.collaboration.subagentStarts.value })
                      : t("vcti.notRecorded")}</strong>
                  </div>
                  <div>
                    <span>{t("vcti.parallelBatchesLabel")}</span>
                    <strong>{profile.identityEvidence.collaboration.parallelBatches.available && profile.identityEvidence.collaboration.parallelBatches.value !== undefined
                      ? t("vcti.countValue", { value: profile.identityEvidence.collaboration.parallelBatches.value })
                      : t("vcti.notRecorded")}</strong>
                  </div>
                  <div>
                    <span>{t("vcti.toolCategoriesLabel")}</span>
                    <strong>{profile.identityEvidence.detailDiversity.toolCategories.available && profile.identityEvidence.detailDiversity.toolCategories.value !== undefined
                      ? t("vcti.categoryCountValue", { value: profile.identityEvidence.detailDiversity.toolCategories.value })
                      : t("vcti.notRecorded")}</strong>
                  </div>
                  <div>
                    <span>{t("vcti.explicitSkillsLabel")}</span>
                    <strong>{profile.identityEvidence.detailDiversity.explicitSkills.available && profile.identityEvidence.detailDiversity.explicitSkills.value !== undefined
                      ? t("vcti.categoryCountValue", { value: profile.identityEvidence.detailDiversity.explicitSkills.value })
                      : t("vcti.notRecorded")}</strong>
                  </div>
                  {([
                    ["errors", "errorsLabel"],
                    ["retries", "retriesLabel"],
                    ["rollbacks", "rollbacksLabel"],
                  ] as const).map(([key, label]) => {
                    const metric = profile.identityEvidence.processVariation[key];
                    return (
                      <div key={key}>
                        <span>{t(`vcti.${label}`)}</span>
                        <strong>{metric.available && metric.value !== undefined
                          ? t("vcti.countValue", { value: metric.value })
                          : t("vcti.notRecorded")}</strong>
                      </div>
                    );
                  })}
                </div>
              </section>
              <section>
                <h3>{t("vcti.coverageTitle")}</h3>
                <p>{t("vcti.coverageBody")}</p>
                <BehaviorCoverage profile={profile} />
                {profile.missingCapabilities.length ? (
                  <div className="vcti-capability-note">
                    <strong>{t("vcti.missingCapability", { count: profile.missingCapabilities.length })}</strong>
                    <span>{profile.missingCapabilities.map((capability) => t(`vcti.capabilityNames.${capability}`)).join(" · ")}</span>
                  </div>
                ) : <p className="vcti-capability-note complete">{t("vcti.coverageComplete")}</p>}
              </section>
              <section>
                <h3>{t("vcti.trendTitle")}</h3>
                <p>{t("vcti.trendBody")}</p>
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
          </section>
        </div>,
        document.body,
      ) : null}

      {showAtlas ? createPortal(
        <div className="modal-backdrop vcti-atlas-backdrop" role="presentation" onMouseDown={(event) => {
          if (event.target === event.currentTarget) setShowAtlas(false);
        }}>
          <section ref={atlasTrapRef} className="vcti-atlas-window" role="dialog" aria-modal="true" aria-labelledby="vcti-atlas-title">
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
              <section className="vcti-atlas-guild vcti-atlas-badges">
                <header>
                  <span className="section-index">07</span>
                  <div><strong>{t("vcti.badgeAtlasTitle")}</strong><small>{t("vcti.badgeAtlasBody")}</small></div>
                </header>
                <div className="vcti-atlas-badge-grid">
                  {VCTI_BADGES.map((code) => {
                    const earned = earnedBadges.has(code);
                    return (
                      <article className={earned ? "earned" : ""} key={code}>
                        <b>{code}</b>
                        <div>
                          <span className="vcti-atlas-badge-heading">
                            <strong>{t(`vcti.badges.${code}.name`)}</strong>
                            {earned ? <small>{t("vcti.badgeEarned")}</small> : null}
                          </span>
                          <p>{t(`vcti.badges.${code}.description`)}</p>
                        </div>
                      </article>
                    );
                  })}
                </div>
              </section>
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
