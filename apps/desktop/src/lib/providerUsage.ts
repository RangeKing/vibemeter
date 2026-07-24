import type { ProviderAccountUsage, ProviderDailyAccountUsage, RangeKey } from "../types";

const RANGE_DAY_COUNTS: Partial<Record<RangeKey, number>> = {
  today: 1,
  "7d": 7,
  "30d": 30,
  "90d": 90,
  "180d": 180,
  year: 365,
};

export interface ProviderAccountDailySummary {
  date: string;
  tokens: number;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  apiCostUsd?: number;
  meteredCostUsd?: number;
  requestCount: number;
  tokenRequestCount: number;
}

export interface ProviderAccountRangeSummary {
  periodStart: string;
  periodEnd: string;
  fetchedAt: string;
  totalTokens: number;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  apiCostUsd?: number;
  meteredCostUsd?: number;
  requestCount: number;
  tokenRequestCount: number;
  daily: ProviderAccountDailySummary[];
  models: Array<{ model: string; tokens: number; requestCount: number }>;
}

export interface ProviderUsageBucket {
  startDate: string;
  endDate: string;
  tokens: number;
}

export function localDateKey(date: Date): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function dateFromLocalKey(value: string): Date | undefined {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) return undefined;
  const date = new Date(Number(match[1]), Number(match[2]) - 1, Number(match[3]));
  return Number.isNaN(date.getTime()) ? undefined : date;
}

function rangeBounds(range: RangeKey, referenceTime: Date): { start: string; end: string } {
  const end = new Date(referenceTime);
  end.setHours(0, 0, 0, 0);
  const start = new Date(end);
  const dayCount = RANGE_DAY_COUNTS[range];
  if (dayCount) start.setDate(start.getDate() - dayCount + 1);
  else start.setFullYear(1970, 0, 1);
  return { start: localDateKey(start), end: localDateKey(end) };
}

function rowTokens(row: ProviderDailyAccountUsage): number {
  return row.inputTokens + row.outputTokens + row.cacheReadTokens + row.cacheWriteTokens;
}

export function summarizeProviderAccountUsage(
  usage: ProviderAccountUsage,
  range: RangeKey,
  referenceTime = new Date(),
): ProviderAccountRangeSummary {
  const requested = rangeBounds(range, referenceTime);
  const start = requested.start > usage.periodStart ? requested.start : usage.periodStart;
  const end = requested.end < usage.periodEnd ? requested.end : usage.periodEnd;
  const rows = start <= end
    ? usage.daily.filter((row) => row.date >= start && row.date <= end)
    : [];
  const grouped = new Map<string, ProviderAccountDailySummary>();
  const models = new Map<string, { model: string; tokens: number; requestCount: number }>();
  let apiCostComplete = true;
  let meteredCostComplete = true;
  for (const row of rows) {
    const current = grouped.get(row.date) ?? {
      date: row.date,
      tokens: 0,
      inputTokens: 0,
      outputTokens: 0,
      cacheReadTokens: 0,
      cacheWriteTokens: 0,
      apiCostUsd: 0,
      meteredCostUsd: 0,
      requestCount: 0,
      tokenRequestCount: 0,
    };
    const tokens = rowTokens(row);
    current.tokens += tokens;
    current.inputTokens += row.inputTokens;
    current.outputTokens += row.outputTokens;
    current.cacheReadTokens += row.cacheReadTokens;
    current.cacheWriteTokens += row.cacheWriteTokens;
    current.requestCount += row.requestCount;
    current.tokenRequestCount += row.tokenRequestCount;
    if (row.tokenRequestCount > 0 && row.apiCostUsd == null) {
      apiCostComplete = false;
      current.apiCostUsd = undefined;
    } else if (current.apiCostUsd !== undefined) {
      current.apiCostUsd += row.apiCostUsd ?? 0;
    }
    if (row.requestCount > 0 && row.meteredCostUsd == null) {
      meteredCostComplete = false;
      current.meteredCostUsd = undefined;
    } else if (current.meteredCostUsd !== undefined) {
      current.meteredCostUsd += row.meteredCostUsd ?? 0;
    }
    grouped.set(row.date, current);
    const model = models.get(row.model) ?? { model: row.model, tokens: 0, requestCount: 0 };
    model.tokens += tokens;
    model.requestCount += row.requestCount;
    models.set(row.model, model);
  }

  if (start <= end) {
    const cursor = dateFromLocalKey(start);
    const last = dateFromLocalKey(end);
    if (cursor && last) {
      while (cursor <= last) {
        const date = localDateKey(cursor);
        if (!grouped.has(date)) {
          grouped.set(date, {
            date,
            tokens: 0,
            inputTokens: 0,
            outputTokens: 0,
            cacheReadTokens: 0,
            cacheWriteTokens: 0,
            apiCostUsd: 0,
            meteredCostUsd: 0,
            requestCount: 0,
            tokenRequestCount: 0,
          });
        }
        cursor.setDate(cursor.getDate() + 1);
      }
    }
  }

  const daily = [...grouped.values()].sort((left, right) => left.date.localeCompare(right.date));
  const totals = daily.reduce(
    (result, day) => {
      result.totalTokens += day.tokens;
      result.inputTokens += day.inputTokens;
      result.outputTokens += day.outputTokens;
      result.cacheReadTokens += day.cacheReadTokens;
      result.cacheWriteTokens += day.cacheWriteTokens;
      result.apiCostUsd += day.apiCostUsd ?? 0;
      result.meteredCostUsd += day.meteredCostUsd ?? 0;
      result.requestCount += day.requestCount;
      result.tokenRequestCount += day.tokenRequestCount;
      return result;
    },
    {
      totalTokens: 0,
      inputTokens: 0,
      outputTokens: 0,
      cacheReadTokens: 0,
      cacheWriteTokens: 0,
      apiCostUsd: 0,
      meteredCostUsd: 0,
      requestCount: 0,
      tokenRequestCount: 0,
    },
  );
  return {
    periodStart: start,
    periodEnd: end,
    fetchedAt: usage.fetchedAt,
    totalTokens: totals.totalTokens,
    inputTokens: totals.inputTokens,
    outputTokens: totals.outputTokens,
    cacheReadTokens: totals.cacheReadTokens,
    cacheWriteTokens: totals.cacheWriteTokens,
    apiCostUsd: apiCostComplete ? totals.apiCostUsd : undefined,
    meteredCostUsd: meteredCostComplete ? totals.meteredCostUsd : undefined,
    requestCount: totals.requestCount,
    tokenRequestCount: totals.tokenRequestCount,
    daily,
    models: [...models.values()].sort((left, right) => right.tokens - left.tokens || right.requestCount - left.requestCount),
  };
}

export function compactProviderUsageBuckets(
  daily: ProviderAccountDailySummary[],
  maxBuckets = 56,
): ProviderUsageBucket[] {
  if (!daily.length) return [];
  const bucketSize = Math.max(1, Math.ceil(daily.length / maxBuckets));
  const buckets: ProviderUsageBucket[] = [];
  for (let index = 0; index < daily.length; index += bucketSize) {
    const rows = daily.slice(index, index + bucketSize);
    buckets.push({
      startDate: rows[0].date,
      endDate: rows[rows.length - 1].date,
      tokens: rows.reduce((sum, row) => sum + row.tokens, 0),
    });
  }
  return buckets;
}
