import { Bot, CheckCircle2, GitPullRequestDraft, ListChecks, ScanSearch } from "lucide-react";
import { useTranslation } from "react-i18next";
import { formatCompact, formatDuration, formatPercent } from "../lib/format";
import type { BehaviorSummary, Locale } from "../types";

const STREAMS = [
  ["structure", ScanSearch],
  ["lifecycle", ListChecks],
  ["orchestration", Bot],
  ["toolResults", CheckCircle2],
  ["processControl", GitPullRequestDraft],
] as const;

function available(value: number | undefined): value is number {
  return value !== undefined && Number.isFinite(value);
}

export function BehaviorStreams({ data, locale, compact = false }: { data: BehaviorSummary; locale: Locale; compact?: boolean }) {
  const { t } = useTranslation();
  const values = {
    structure: {
      primary: available(data.structuredPromptRate) ? formatPercent(data.structuredPromptRate, locale) : "—",
      secondary: available(data.acceptanceCriteriaRate) ? t("behavior.structure.detail", { value: formatPercent(data.acceptanceCriteriaRate, locale) }) : t("metrics.unavailable"),
      coverage: data.structureCoverage,
    },
    lifecycle: {
      primary: available(data.completionRate) ? formatPercent(data.completionRate, locale) : "—",
      secondary: t("behavior.lifecycle.detail", { completed: data.taskCompletions, aborted: data.taskAborts }),
      coverage: data.lifecycleCoverage,
    },
    orchestration: {
      primary: formatCompact(data.subagentStarts, locale),
      secondary: t("behavior.orchestration.detail", { interactions: data.subagentInteractions, batches: data.parallelBatches }),
      coverage: data.orchestrationCoverage,
    },
    toolResults: {
      primary: available(data.toolSuccessRate) ? formatPercent(data.toolSuccessRate, locale) : "—",
      secondary: t("behavior.toolResults.detail", { success: data.successfulTools, failed: data.failedTools }),
      coverage: data.toolResultCoverage,
    },
    processControl: {
      primary: formatCompact(data.planEvents, locale),
      secondary: t("behavior.processControl.detail", { rollbacks: data.rollbacks, compactions: data.contextCompactions }),
      coverage: data.processControlCoverage,
    },
  };
  return (
    <div className={`behavior-streams ${compact ? "compact" : ""}`}>
      {STREAMS.map(([id, Icon], index) => {
        const item = values[id];
        return (
          <article key={id} className={item.coverage < .5 ? "low-coverage" : ""}>
            <header><span>{String(index + 1).padStart(2, "0")}</span><Icon size={16} /></header>
            <h3>{t(`behavior.${id}.title`)}</h3>
            <strong>{item.primary}</strong>
            <p>{item.secondary}</p>
            {!compact && id === "lifecycle" && available(data.averageTaskDurationSeconds) ? <small>{t("behavior.lifecycle.duration", { value: formatDuration(Math.round(data.averageTaskDurationSeconds), locale) })}</small> : null}
          </article>
        );
      })}
    </div>
  );
}
