import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { BookOpenCheck, Check, CheckCircle2, CircleHelp, CircleMinus, Database, LockKeyhole, RadioTower, RefreshCw, Settings2, ShieldCheck, Slash } from "lucide-react";
import { useTranslation } from "react-i18next";
import { AgentBadge, ErrorState, LoadingState, PageHeader } from "../components/ui";
import { api } from "../lib/api";
import { formatCompact, formatDateTime } from "../lib/format";
import { capabilityTranslationKey, sourceLiveTranslationKey } from "../lib/sourceStatus";
import type { Locale, SourceStatus } from "../types";

export function SourcesPage({ locale }: { locale: Locale }) {
  const { t } = useTranslation();
  const client = useQueryClient();
  const sources = useQuery({ queryKey: ["sources"], queryFn: api.sources, refetchInterval: 30_000 });
  const live = useQuery({ queryKey: ["live-snapshot"], queryFn: api.liveSnapshot, refetchInterval: 5_000 });
  const index = useQuery({ queryKey: ["index-status"], queryFn: api.indexStatus, refetchInterval: 1_500 });
  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  const selection = useMutation({
    mutationFn: ({ agent, selected }: { agent: string; selected: boolean }) =>
      api.setSourceSelected(agent, selected),
    onMutate: async ({ agent, selected }) => {
      await client.cancelQueries({ queryKey: ["sources"] });
      const previous = client.getQueryData<SourceStatus[]>(["sources"]);
      client.setQueryData<SourceStatus[]>(["sources"], (current) =>
        current?.map((source) =>
          source.agent === agent ? { ...source, selected } : source,
        ),
      );
      return { previous };
    },
    onError: (_error, _variables, context) => {
      if (context?.previous) client.setQueryData(["sources"], context.previous);
    },
    onSettled: async () => {
      await Promise.all([
        client.invalidateQueries({ queryKey: ["sources"] }),
        client.invalidateQueries({ queryKey: ["overview"] }),
        client.invalidateQueries({ queryKey: ["tasks"] }),
      ]);
    },
  });
  const refresh = async () => { await api.refreshIndex(true); await Promise.all([client.invalidateQueries({ queryKey: ["index-status"] }), client.invalidateQueries({ queryKey: ["sources"] })]); };
  return (
    <div className="page sources-page">
      <PageHeader title={t("sources.title")} description={t("sources.description")} actions={<button className="button secondary" onClick={() => void refresh()} disabled={index.data?.running}><RefreshCw size={14} className={index.data?.running ? "spin" : ""} />{index.data?.running ? t("actions.refreshing") : t("actions.refresh")}</button>} />
      {index.data ? <section className="index-banner"><span className={index.data.running ? "pulse-dot" : "ready-dot"} /><div><strong>{t(index.data.messageKey)}</strong><span>{index.data.running ? t("sources.indexProgress", { processed: index.data.processedFiles, total: index.data.discoveredFiles }) : t("settings.projectSessions", { count: index.data.indexedSessions })}</span></div>{index.data.discoveredFiles > 0 ? <div className="index-progress"><span style={{ width: `${(index.data.processedFiles / index.data.discoveredFiles) * 100}%` }} /></div> : null}</section> : null}
      {sources.isLoading ? <LoadingState /> : sources.isError || !sources.data ? <ErrorState retry={() => void sources.refetch()} /> : <div className="source-grid">{sources.data.map((source, index) => {
        const liveProvider = live.data?.hookStatus.providers.find((provider) => provider.provider === source.agent);
        const liveIntegrationReady = live.data ? Boolean(liveProvider?.installed) : undefined;
        const selected = source.available && source.selected;
        const readIssue = source.available && (source.status === "unavailable" || source.status === "partial");
        const cursorAccountDisabled = source.agent === "cursor" && settings.data?.cursorDashboardUsage === "false";
        return <section className={`source-card capability-${source.capabilityLevel} ${selected ? "is-selected" : "is-unselected"} ${source.available ? "" : "is-missing"}`} key={source.agent}>
          <span className="source-number">{String(index + 1).padStart(2, "0")}</span>
          <header>
            <AgentBadge agent={source.agent} />
            <span className={`source-state ${readIssue ? "unavailable" : source.available ? "available" : "missing"}`}>
              {readIssue ? <CircleHelp size={13} /> : source.available ? <CheckCircle2 size={13} /> : <Slash size={13} />}
              {t(source.status === "partial" ? "sources.partiallyReadable" : readIssue ? "sources.readUnavailable" : source.available ? "sources.available" : "sources.notFound")}
            </span>
          </header>
          <label className={`source-selector ${selected ? "selected" : ""} ${source.available ? "" : "disabled"}`}>
            <input
              type="checkbox"
              checked={selected}
              disabled={!source.available || selection.isPending}
              onChange={(event) => selection.mutate({ agent: source.agent, selected: event.target.checked })}
            />
            <span className="source-selector-box"><Check size={11} strokeWidth={2.8} /></span>
            <span>{t(selected ? "sources.included" : source.available ? "sources.excluded" : "sources.unavailableSelection")}</span>
          </label>
          <div className="source-count"><strong>{formatCompact(source.sessionCount, locale)}</strong><span>{t("metrics.sessions")}</span></div>
          <div className="source-capability"><span>{t("sources.readability")}</span><strong>{t(capabilityTranslationKey(source.capabilityLevel, source.available))}</strong></div>
          <div className={`source-live ${liveProvider?.installed ? "ready" : ""}`}>
            <RadioTower size={13} />
            <span>{t("sources.live")}</span>
            <strong>{t(sourceLiveTranslationKey(source.liveCapability, liveIntegrationReady))}</strong>
          </div>
          {cursorAccountDisabled ? <div className="source-account-warning">
            <LockKeyhole size={14} />
            <div><strong>{t("cursorUsage.disabledTitle")}</strong><span>{t("sources.cursorAccountBody")}</span></div>
            <button onClick={() => void api.showSettings()} aria-label={t("cursorUsage.openSettings")}><Settings2 size={12} />{t("sources.enableCursorAccount")}</button>
          </div> : null}
          <footer><Database size={13} /><span>{source.lastIndexedAt ? t("sources.lastIndexed", { time: formatDateTime(source.lastIndexedAt, locale) }) : t("sources.notIndexed")} · {t("sources.parserVersion", { version: source.parserVersion })}</span></footer>
        </section>;
      })}</div>}
      <section className="source-glossary" aria-labelledby="source-glossary-title">
        <header>
          <span><BookOpenCheck size={18} /></span>
          <div>
            <h2 id="source-glossary-title">{t("sources.glossaryTitle")}</h2>
            <p>{t("sources.glossaryBody")}</p>
          </div>
        </header>
        <div className="source-term-grid">
          <article className="source-term verified">
            <CheckCircle2 size={18} />
            <div><strong>{t("sources.verificationTerms.verified.label")}</strong><p>{t("sources.verificationTerms.verified.body")}</p></div>
          </article>
          <article className="source-term unverified">
            <CircleHelp size={18} />
            <div><strong>{t("sources.verificationTerms.unverified.label")}</strong><p>{t("sources.verificationTerms.unverified.body")}</p></div>
          </article>
          <article className="source-term not-applicable">
            <CircleMinus size={18} />
            <div><strong>{t("sources.verificationTerms.notApplicable.label")}</strong><p>{t("sources.verificationTerms.notApplicable.body")}</p></div>
          </article>
        </div>
      </section>
      <section className="privacy-panel"><span className="privacy-icon"><ShieldCheck size={21} /></span><div><h2>{t("sources.privacyTitle")}</h2><p>{t("sources.privacyBody")}</p></div></section>
    </div>
  );
}
