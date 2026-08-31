import capabilityRegistry from "../../source-capabilities.json";
import type {
  DistributionItem,
  SignalCapability,
  SourceCapabilitiesV2,
  SourceLiveCapability,
  SourceStatus,
} from "../types";

type SourceHistoryCapability = "full" | "partial";

export type SourceCapabilityEntry = {
  agent: string;
  displayName: string;
  historyCapability: SourceHistoryCapability;
  liveCapability: SourceLiveCapability;
  jumpSupported: boolean;
  signals: SourceCapabilitiesV2;
};

const signalKeys = [
  "history",
  "liveLifecycle",
  "jump",
  "delegation",
  "handoff",
  "subagent",
  "memoryRead",
  "memoryWrite",
  "skillUse",
  "evaluation",
] as const satisfies readonly (keyof SourceCapabilitiesV2)[];

function parseSignalCapability(value: unknown, key: string): SignalCapability {
  if (value === "exact" || value === "derived" || value === "partial" || value === "unavailable") {
    return value;
  }
  throw new Error(`Unknown ${key} capability: ${String(value)}`);
}

function historyCapability(value: SignalCapability): SourceHistoryCapability {
  return value === "exact" || value === "derived" ? "full" : "partial";
}

function liveCapability(value: SignalCapability): SourceLiveCapability {
  if (value === "exact") return "exact";
  if (value === "unavailable") return "none";
  return "experimental";
}

export function parseSourceCapabilities(registry: unknown): SourceCapabilityEntry[] {
  if (
    !registry
    || typeof registry !== "object"
    || !("version" in registry)
    || registry.version !== 2
    || !("sources" in registry)
    || !Array.isArray(registry.sources)
  ) {
    throw new Error("Invalid source capability registry");
  }
  const agents = new Set<string>();
  return registry.sources.map((value) => {
    if (!value || typeof value !== "object") throw new Error("Invalid source capability entry");
    const source = value as Record<string, unknown>;
    if (typeof source.agent !== "string" || !source.agent || typeof source.displayName !== "string" || !source.displayName) {
      throw new Error("Invalid source capability entry");
    }
    if (agents.has(source.agent)) throw new Error(`Duplicate source capability: ${source.agent}`);
    agents.add(source.agent);
    const signals = Object.fromEntries(
      signalKeys.map((key) => [key, parseSignalCapability(source[key], key)]),
    ) as unknown as SourceCapabilitiesV2;
    return {
      agent: source.agent,
      displayName: source.displayName,
      historyCapability: historyCapability(signals.history),
      liveCapability: liveCapability(signals.liveLifecycle),
      jumpSupported: signals.jump !== "unavailable",
      signals,
    };
  });
}

export const sourceCapabilities = parseSourceCapabilities(capabilityRegistry);

export const DATA_PAGE_AGENTS_AUTO = "auto";

export function sourceSignalCapability(
  agent: string,
  signal: keyof SourceCapabilitiesV2,
): SignalCapability {
  return sourceCapabilities.find((source) => source.agent === agent)?.signals[signal] ?? "unavailable";
}

export function signalCapabilityTranslationKey(capability: SignalCapability): string {
  return `sources.signalCapabilities.${capability}`;
}

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
