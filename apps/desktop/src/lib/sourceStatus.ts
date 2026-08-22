import capabilityRegistry from "../../source-capabilities.json";
import type { DistributionItem, SourceLiveCapability, SourceStatus } from "../types";

type SourceHistoryCapability = "full" | "partial";

type SourceCapabilityEntry = {
  agent: string;
  displayName: string;
  historyCapability: SourceHistoryCapability;
  liveCapability: SourceLiveCapability;
  jumpSupported: boolean;
};

export function parseSourceCapabilities(registry: unknown): SourceCapabilityEntry[] {
  if (!registry || typeof registry !== "object" || !("sources" in registry) || !Array.isArray(registry.sources)) {
    throw new Error("Invalid source capability registry");
  }
  return registry.sources.map((value) => {
    if (!value || typeof value !== "object") throw new Error("Invalid source capability entry");
    const source = value as Record<string, unknown>;
    const historyCapability = source.historyCapability;
    const liveCapability = source.liveCapability;
    if (historyCapability !== "full" && historyCapability !== "partial") {
      throw new Error(`Unknown history capability: ${String(historyCapability)}`);
    }
    if (liveCapability !== "exact" && liveCapability !== "experimental" && liveCapability !== "none") {
      throw new Error(`Unknown live capability: ${String(liveCapability)}`);
    }
    if (typeof source.agent !== "string" || typeof source.displayName !== "string" || typeof source.jumpSupported !== "boolean") {
      throw new Error("Invalid source capability entry");
    }
    return {
      agent: source.agent,
      displayName: source.displayName,
      historyCapability,
      liveCapability,
      jumpSupported: source.jumpSupported,
    };
  });
}

export const sourceCapabilities = parseSourceCapabilities(capabilityRegistry);

export const DATA_PAGE_AGENTS_AUTO = "auto";

export function sourceNamesForLiveCapability(capability: SourceLiveCapability): string[] {
  return sourceCapabilities
    .filter((source) => source.liveCapability === capability)
    .map((source) => source.displayName);
}

export function sourceCapabilityNameGroups(separator = "、") {
  return {
    exact: sourceNamesForLiveCapability("exact").join(separator),
    experimental: sourceNamesForLiveCapability("experimental").join(separator),
    historyOnly: sourceNamesForLiveCapability("none").join(separator),
  };
}

export function sourceLiveTranslationKey(
  capability: SourceLiveCapability,
  integrationReady: boolean | undefined,
): string {
  if (capability === "none") return "sources.liveCapabilities.historyOnly";
  if (integrationReady === undefined) return "sources.liveCapabilities.unknown";
  if (capability === "exact") {
    return integrationReady
      ? "sources.liveCapabilities.exactReady"
      : "sources.liveCapabilities.exactNeedsSetup";
  }
  return integrationReady
    ? "sources.liveCapabilities.experimentalReady"
    : "sources.liveCapabilities.experimentalUnavailable";
}

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

export function detectedDataAgents(sources: SourceStatus[]): string[] {
  return sources
    .filter((source) => source.available)
    .map((source) => source.agent);
}

export function parseDataPageAgents(value: string | undefined): string[] | undefined {
  if (!value || value === DATA_PAGE_AGENTS_AUTO) return undefined;
  try {
    const parsed: unknown = JSON.parse(value);
    if (!Array.isArray(parsed) || parsed.some((agent) => typeof agent !== "string")) return undefined;
    return [...new Set(parsed)];
  } catch {
    return undefined;
  }
}

export function serializeDataPageAgents(agents: string[]): string {
  return JSON.stringify([...new Set(agents)].sort());
}

export function dataFilterAgents(sources: SourceStatus[], configuredAgents?: string[]): string[] {
  const allowed = configuredAgents ? new Set(configuredAgents) : undefined;
  return detectedDataAgents(sources).filter((agent) => !allowed || allowed.has(agent));
}
