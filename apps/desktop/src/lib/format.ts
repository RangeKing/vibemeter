import type { Locale, TokenUsage } from "../types";

export function tokenTotal(usage: TokenUsage): number {
  return usage.inputTokens + usage.outputTokens + usage.cacheReadTokens + usage.cacheWriteTokens + usage.cacheWrite1hTokens;
}

export function cacheTokenTotal(usage: TokenUsage): number {
  return usage.cacheReadTokens + usage.cacheWriteTokens + usage.cacheWrite1hTokens;
}

export function sumTokenUsage(usages: TokenUsage[]): TokenUsage {
  return usages.reduce<TokenUsage>((total, usage) => ({
    inputTokens: total.inputTokens + usage.inputTokens,
    outputTokens: total.outputTokens + usage.outputTokens,
    cacheReadTokens: total.cacheReadTokens + usage.cacheReadTokens,
    cacheWriteTokens: total.cacheWriteTokens + usage.cacheWriteTokens,
    cacheWrite1hTokens: total.cacheWrite1hTokens + usage.cacheWrite1hTokens,
    reasoningTokens: total.reasoningTokens + usage.reasoningTokens,
  }), {
    inputTokens: 0,
    outputTokens: 0,
    cacheReadTokens: 0,
    cacheWriteTokens: 0,
    cacheWrite1hTokens: 0,
    reasoningTokens: 0,
  });
}

export function formatCompact(value: number, locale: Locale): string {
  return new Intl.NumberFormat(locale, { notation: value >= 1_000 ? "compact" : "standard", maximumFractionDigits: 1 }).format(value);
}

export function formatDate(value: string, locale: Locale, style: "short" | "medium" = "medium"): string {
  const date = /^\d{4}-\d{2}-\d{2}$/.test(value) ? new Date(`${value}T12:00:00`) : new Date(value);
  return new Intl.DateTimeFormat(locale, { dateStyle: style }).format(date);
}

export function formatDateTime(value: string | null | undefined, locale: Locale): string {
  if (!value) return "—";
  return new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "short" }).format(new Date(value));
}

export function formatTime(value: string, locale: Locale): string {
  return new Intl.DateTimeFormat(locale, { hour: "2-digit", minute: "2-digit" }).format(new Date(value));
}

export function formatQuarter(value: string, locale: Locale): string {
  const date = /^\d{4}-\d{2}-\d{2}$/.test(value) ? new Date(`${value}T12:00:00`) : new Date(value);
  const quarter = Math.floor(date.getMonth() / 3) + 1;
  const year = String(date.getFullYear()).slice(-2);
  return locale === "zh-CN" ? `${year} 年 Q${quarter}` : `Q${quarter} ’${year}`;
}

export function formatCurrency(value: number, locale: Locale): string {
  return new Intl.NumberFormat(locale, { style: "currency", currency: "USD", maximumFractionDigits: 2 }).format(value);
}

export function formatPercent(value: number, locale: Locale): string {
  return new Intl.NumberFormat(locale, { style: "percent", maximumFractionDigits: 0 }).format(value);
}

export function formatDuration(seconds: number, locale: Locale): string {
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (locale === "zh-CN") {
    if (hours > 0) return minutes > 0 ? `${hours} 小时 ${minutes} 分` : `${hours} 小时`;
    return minutes > 0 ? `${minutes} 分钟` : `${seconds} 秒`;
  }
  if (hours > 0) return minutes > 0 ? `${hours}h ${minutes}m` : `${hours}h`;
  return minutes > 0 ? `${minutes}m` : `${seconds}s`;
}

export function agentName(value: string): string {
  if (value === "claude-code") return "Claude Code";
  if (value === "codex") return "Codex";
  if (value === "deepseek-harness") return "DeepSeek Harness";
  if (value === "kimi-code") return "Kimi Code";
  if (value === "grok-build") return "Grok Build";
  if (value === "cursor") return "Cursor";
  if (value === "openclaw") return "OpenClaw";
  if (value === "hermes") return "Hermes";
  if (value === "zcode") return "ZCode";
  if (value === "vibemeter") return "VibeMeter";
  return value;
}
