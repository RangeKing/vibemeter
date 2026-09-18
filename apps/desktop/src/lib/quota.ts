import type {
  CreditBalance,
  Locale,
  ProviderAccount,
  ProviderUsage,
  RateWindow,
  SourceStatus,
} from "../types";

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
  /** Headline balance, where the provider bills a key rather than a plan. */
  balance?: CreditBalance;
}

/**
 * Condenses a provider into the one number a ring can carry: the window with
 * the least left, since that is the one that will stop you first. Windows the
 * provider did not report stay out of the calculation rather than counting as
 * full — an unreported quota is unknown, not untouched.
 */
export function quotaSummary(provider: ProviderUsage): QuotaSummary {
  return accountsSummary(provider.available ? provider.accounts : []);
}

/**
 * Adds up what several keys hold.
 *
 * Money combines, so two keys on one provider are one figure: what is left to
 * spend. Mixed currencies do not combine, so the largest holding is reported
 * on its own rather than summing figures that are not comparable.
 */
export function combineBalances(accounts: ProviderAccount[]): CreditBalance | undefined {
  const balances = accounts
    .filter((account) => account.available)
    .map((account) => account.balance)
    .filter((balance): balance is CreditBalance => Boolean(balance));
  if (!balances.length) return undefined;
  const currencies = new Set(balances.map((balance) => balance.currency));
  if (currencies.size > 1) {
    return [...balances].sort((left, right) => right.total - left.total)[0];
  }
  const sum = (pick: (balance: CreditBalance) => number | undefined) => {
    const values = balances.map(pick).filter((value): value is number => value !== undefined);
    return values.length ? values.reduce((total, value) => total + value, 0) : undefined;
  };
  return {
    currency: balances[0].currency,
    total: balances.reduce((total, balance) => total + balance.total, 0),
    granted: sum((balance) => balance.granted),
    toppedUp: sum((balance) => balance.toppedUp),
    // Blocked on any one key is worth surfacing: that key has stopped working
    // even though the combined figure still looks healthy.
    spendable: balances.some((balance) => balance.spendable === false)
      ? false
      : balances.every((balance) => balance.spendable === true)
        ? true
        : undefined,
    provenance: balances[0].provenance,
  };
}

/**
 * Condenses every account of a provider into the one reading a ring can carry.
 *
 * Windows win over balances when both exist, because a plan window is the
 * limit that actually stops work; a balance has no denominator, so it can only
 * be reported as an amount. Accounts do not share a limit, so the window taken
 * is the tightest across all of them.
 */
export function accountsSummary(accounts: ProviderAccount[]): QuotaSummary {
  const live = accounts.filter((account) => account.available);
  const windows = live.flatMap((account) => account.windows);
  const measured = windows
    .map((window) => window.usedPercent)
    .filter((value): value is number => value !== undefined && Number.isFinite(value));
  const balance = combineBalances(live);
  if (!measured.length) {
    return {
      // No percentage to report, but an exhausted or blocked key still needs
      // to read as a problem rather than as an absence of data.
      band:
        balance === undefined
          ? "unknown"
          : balance.spendable === false || balance.total <= 0
            ? "critical"
            : "steady",
      windows,
      balance,
    };
  }
  const used = Math.min(100, Math.max(0, Math.max(...measured)));
  const remainingPercent = 100 - used;
  return {
    remainingPercent,
    band: remainingPercent < 20 ? "critical" : remainingPercent < 50 ? "warning" : "steady",
    windows,
    balance,
  };
}

/** A compact amount for a ring caption: "¥110", "$8.5". */
export function formatBalanceShort(balance: CreditBalance, locale: Locale): string {
  const symbol = balance.currency === "CNY" ? "¥" : balance.currency === "USD" ? "$" : "";
  const value = new Intl.NumberFormat(locale, {
    notation: Math.abs(balance.total) >= 1000 ? "compact" : "standard",
    maximumFractionDigits: Math.abs(balance.total) >= 100 ? 0 : 1,
  }).format(balance.total);
  return symbol ? `${symbol}${value}` : `${value} ${balance.currency}`;
}

export function formatBalance(balance: CreditBalance, locale: Locale): string {
  return new Intl.NumberFormat(locale, {
    style: "currency",
    currency: balance.currency,
    maximumFractionDigits: 2,
  }).format(balance.total);
}

/** What a ring says when it has no percentage to show. */
export function ringCaption(summary: QuotaSummary, locale: Locale): string {
  if (summary.remainingPercent !== undefined) {
    return `${Math.round(summary.remainingPercent)}%`;
  }
  return summary.balance ? formatBalanceShort(summary.balance, locale) : "—";
}

export const EDGE_AGENTS_AUTO = "auto";

/** Reads the stored choice. `undefined` means "every detected agent", not "none". */
export function parseEdgeAgents(value: string | undefined): string[] | undefined {
  if (!value || value === EDGE_AGENTS_AUTO) return undefined;
  try {
    const parsed: unknown = JSON.parse(value);
    if (!Array.isArray(parsed) || parsed.some((item) => typeof item !== "string")) return undefined;
    return [...new Set(parsed as string[])];
  } catch {
    return undefined;
  }
}

export function serializeEdgeAgents(agents: string[]): string {
  return JSON.stringify([...new Set(agents)].sort());
}

/**
 * The provider an agent bills against, where VibeMeter can read something.
 *
 * Two kinds sit behind this: a plan whose windows reset, and a key whose
 * balance does not. Agents with neither still get a ring when ticked — they
 * are agents the app supports, and leaving them out would say more than the
 * data does — but the ring carries no arc and the card says so.
 */
export function agentSubscription(agent: string): string | undefined {
  if (agent === "claude-code") return "claude";
  if (agent === "codex") return "codex";
  if (agent === "cursor") return "cursor";
  if (agent === "deepseek-harness") return "deepseek";
  if (agent === "kimi-code") return "moonshot";
  return undefined;
}

/** Providers billed per key rather than per plan. */
export function isApiProvider(provider: string): boolean {
  return provider === "deepseek" || provider === "moonshot";
}

export interface EdgeAgentQuota {
  agent: string;
  /** Present only when the agent bills against a readable provider. */
  provider?: ProviderUsage;
  /** Every credential the provider was read through, subscription and key. */
  accounts: ProviderAccount[];
  summary: QuotaSummary;
}

/**
 * The rings the sidebar draws, in the sources' own order.
 *
 * `configured` undefined means the automatic list: an Agent detected on this
 * Mac that also has a subscription to read. A local harness with nothing to
 * report would otherwise take a slot to say so forever, which is worth an
 * explicit tick in Settings but not the default. An explicit list is taken as
 * given, including the empty one, so those Agents can still be shown on
 * purpose.
 */
export function edgeAgentQuotas(
  sources: SourceStatus[],
  providers: ProviderUsage[],
  configured: string[] | undefined,
): EdgeAgentQuota[] {
  const allowed = configured ? new Set(configured) : undefined;
  return sources
    .filter((source) =>
      allowed
        ? allowed.has(source.agent)
        : source.available && agentSubscription(source.agent) !== undefined,
    )
    .map((source) => {
      const subscription = agentSubscription(source.agent);
      const provider = subscription
        ? providers.find((item) => item.provider === subscription)
        : undefined;
      const accounts = provider?.accounts ?? [];
      return {
        agent: source.agent,
        provider,
        accounts,
        summary: provider ? accountsSummary(accounts) : { band: "unknown" as const, windows: [] },
      };
    });
}

