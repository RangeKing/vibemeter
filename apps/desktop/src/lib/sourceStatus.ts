import type { DistributionItem, SourceStatus } from "../types";

export function capabilityTranslationKey(level: string, available: boolean): string {
  if (!available) return "sources.capabilities.unavailable";
  if (level === "full") return "sources.capabilities.full";
  if (level === "partial") return "sources.capabilities.partial";
  return "sources.capabilities.basic";
}

export function defaultDataAgents(
  sources: SourceStatus[],
  usage: Pick<DistributionItem, "label" | "value">[],
): string[] {
  const usageByAgent = new Map(usage.map((item) => [item.label, item.value]));
  return sources
    .filter(
      (source) =>
        source.available &&
        source.selected &&
        source.sessionCount > 0 &&
        (usageByAgent.get(source.agent) ?? 0) > 0,
    )
    .map((source) => source.agent);
}
