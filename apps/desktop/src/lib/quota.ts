import type { Locale, ProviderUsage, RateWindow } from "../types";

export function resetTime(window: RateWindow, locale: Locale): string | undefined {
  if (window.resetAt) {
    const date = new Date(window.resetAt);
    if (!Number.isNaN(date.getTime())) {
      return new Intl.DateTimeFormat(locale, {
        month: "short",
        day: "numeric",
        weekday: "short",
        hour: "2-digit",
        minute: "2-digit",
      }).format(date);
    }
  }
  return window.resetDescription;
}

export function resetRemainingSeconds(window: RateWindow, now = new Date()): number | undefined {
  if (!window.resetAt) return undefined;
  const resetAt = new Date(window.resetAt);
  if (Number.isNaN(resetAt.getTime())) return undefined;
  const seconds = Math.ceil((resetAt.getTime() - now.getTime()) / 1000);
  return seconds > 0 ? seconds : undefined;
}

export function formatResetRemaining(seconds: number, locale: Locale): string {
  const totalMinutes = Math.max(1, Math.ceil(seconds / 60));
  const days = Math.floor(totalMinutes / (24 * 60));
  const hours = Math.floor((totalMinutes % (24 * 60)) / 60);
  const minutes = totalMinutes % 60;
  const parts: string[] = [];

  if (days > 0) parts.push(locale === "zh-CN" ? `${days} 天` : `${days}d`);
  if (hours > 0) parts.push(locale === "zh-CN" ? `${hours} 小时` : `${hours}h`);
  if (minutes > 0 && parts.length < 2) parts.push(locale === "zh-CN" ? `${minutes} 分钟` : `${minutes}m`);

  return parts.join(" ") || (locale === "zh-CN" ? "1 分钟" : "1m");
}

/** Display name for a subscription provider. Not an agent: one Anthropic or
    OpenAI subscription backs every agent that signs in with it. */
export function providerDisplayName(provider: string): string {
  if (provider === "claude") return "Claude";
  if (provider === "codex") return "Codex";
  if (provider === "cursor") return "Cursor";
  return provider;
}

/** The agent mark that stands in for a provider in the icon set. */
export function providerAgentIcon(provider: string): string {
  if (provider === "claude") return "claude-code";
  return provider;
}

export type QuotaBand = "critical" | "warning" | "steady" | "unknown";

export interface QuotaSummary {
  /** Remaining share of the tightest measured window, 0-100. */
  remainingPercent?: number;
  band: QuotaBand;
  /** Windows the provider actually reported, in provider order. */
  windows: RateWindow[];
}

/**
 * Condenses a provider into the one number a ring can carry: the window with
 * the least left, since that is the one that will stop you first. Windows the
 * provider did not report stay out of the calculation rather than counting as
 * full — an unreported quota is unknown, not untouched.
 */
export function quotaSummary(provider: ProviderUsage): QuotaSummary {
  const windows = provider.available ? provider.windows : [];
  const measured = windows
    .map((window) => window.usedPercent)
    .filter((value): value is number => value !== undefined && Number.isFinite(value));
  if (!measured.length) return { band: "unknown", windows };
  const used = Math.min(100, Math.max(0, Math.max(...measured)));
  const remainingPercent = 100 - used;
  return {
    remainingPercent,
    band: remainingPercent < 20 ? "critical" : remainingPercent < 50 ? "warning" : "steady",
    windows,
  };
}

export const EDGE_PROVIDERS_AUTO = "auto";

/** Reads the stored choice. `undefined` means "every provider", not "none". */
export function parseEdgeProviders(value: string | undefined): string[] | undefined {
  if (!value || value === EDGE_PROVIDERS_AUTO) return undefined;
  try {
    const parsed: unknown = JSON.parse(value);
    if (!Array.isArray(parsed) || parsed.some((item) => typeof item !== "string")) return undefined;
    return [...new Set(parsed as string[])];
  } catch {
    return undefined;
  }
}

export function serializeEdgeProviders(providers: string[]): string {
  return JSON.stringify([...new Set(providers)].sort());
}

/**
 * Applies the choice while keeping the store's order. An empty stored list is
 * a real answer — show nothing — and stays distinct from no choice at all.
 */
export function visibleQuotaProviders(
  providers: ProviderUsage[],
  configured: string[] | undefined,
): ProviderUsage[] {
  if (!configured) return providers;
  const allowed = new Set(configured);
  return providers.filter((provider) => allowed.has(provider.provider));
}
