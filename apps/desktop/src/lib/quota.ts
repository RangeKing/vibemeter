import type { Locale, RateWindow } from "../types";

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
