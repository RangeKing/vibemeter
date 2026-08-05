import type { DailyUsagePoint, HourlyUsagePoint, RangeKey } from "../types";
import { tokenTotal } from "./format";

export interface ActivityValue {
  codex: number;
  claude: number;
  total: number;
}

export interface ActivityDay extends ActivityValue {
  date: string;
  level: number;
}

export interface ActivityHour extends ActivityValue {
  key: string;
  startAt: string;
  endAt: string;
  level: number;
  granularity: "hour" | "four-hours";
}

export interface PeakActivityPoint {
  period: string;
  total: number;
}

export interface TrendBucket extends ActivityValue {
  startDate: string;
  endDate: string;
  granularity: "day" | "week" | "month" | "quarter";
}

export interface HeatmapLayout {
  columns: number;
  rows: number;
  weekLayout: boolean;
  dense: boolean;
}

const RANGE_DAYS: Record<Exclude<RangeKey, "all">, number> = {
  today: 1,
  "7d": 7,
  "30d": 30,
  "90d": 90,
  "180d": 180,
  year: 365,
};

function parseDate(value: string): Date {
  const [year, month, day] = value.split("-").map(Number);
  return new Date(Date.UTC(year, month - 1, day));
}

function utcDateKey(date: Date): string {
  const year = date.getUTCFullYear();
  const month = String(date.getUTCMonth() + 1).padStart(2, "0");
  const day = String(date.getUTCDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function localDateKey(date: Date): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function localHourKey(date: Date): string {
  return `${localDateKey(date)}T${String(date.getHours()).padStart(2, "0")}:00`;
}

function parseLocalHour(value: string): Date {
  const [date, time = "00:00"] = value.split("T");
  const [year, month, day] = date.split("-").map(Number);
  const [hour, minute] = time.split(":").map(Number);
  return new Date(year, month - 1, day, hour, minute || 0, 0, 0);
}

function addDays(value: string, days: number): string {
  const date = parseDate(value);
  date.setUTCDate(date.getUTCDate() + days);
  return utcDateKey(date);
}

function daysBetween(start: string, end: string): number {
  return Math.max(0, Math.round((parseDate(end).getTime() - parseDate(start).getTime()) / 86_400_000));
}

export function aggregateDaily(points: DailyUsagePoint[]): Map<string, ActivityValue> {
  const values = new Map<string, ActivityValue>();
  for (const point of points) {
    const current = values.get(point.date) ?? { codex: 0, claude: 0, total: 0 };
    const tokens = tokenTotal(point.usage);
    if (point.agent === "claude-code") current.claude += tokens;
    else current.codex += tokens;
    current.total += tokens;
    values.set(point.date, current);
  }
  return values;
}

export function aggregateHourly(points: HourlyUsagePoint[]): Map<string, ActivityValue> {
  const values = new Map<string, ActivityValue>();
  for (const point of points) {
    const current = values.get(point.hour) ?? { codex: 0, claude: 0, total: 0 };
    const tokens = tokenTotal(point.usage);
    if (point.agent === "claude-code") current.claude += tokens;
    else current.codex += tokens;
    current.total += tokens;
    values.set(point.hour, current);
  }
  return values;
}

export function findPeakActivity(points: PeakActivityPoint[]): PeakActivityPoint | null {
  return points.reduce<PeakActivityPoint | null>((peak, point) => {
    if (point.total <= 0 || (peak && point.total <= peak.total)) return peak;
    return point;
  }, null);
}

export function buildHourlyActivity(points: HourlyUsagePoint[], range: "today" | "7d", referenceTime = new Date()): ActivityHour[] {
  const values = aggregateHourly(points);
  const hoursPerCell = range === "today" ? 1 : 4;
  const start = new Date(referenceTime);
  start.setHours(0, 0, 0, 0);
  if (range === "7d") start.setDate(start.getDate() - 6);
  const end = new Date(referenceTime);
  end.setMinutes(0, 0, 0);
  end.setHours(Math.floor(end.getHours() / hoursPerCell) * hoursPerCell);

  const cells: Array<Omit<ActivityHour, "level">> = [];
  for (const cursor = new Date(start); cursor <= end; cursor.setHours(cursor.getHours() + hoursPerCell)) {
    const cellStart = new Date(cursor);
    const cellEnd = new Date(cursor);
    cellEnd.setHours(cellEnd.getHours() + hoursPerCell);
    const value = { codex: 0, claude: 0, total: 0 };
    for (let offset = 0; offset < hoursPerCell; offset += 1) {
      const hour = new Date(cellStart);
      hour.setHours(hour.getHours() + offset);
      const observed = values.get(localHourKey(hour));
      if (!observed) continue;
      value.codex += observed.codex;
      value.claude += observed.claude;
      value.total += observed.total;
    }
    cells.push({
      key: localHourKey(cellStart),
      startAt: localHourKey(cellStart),
      endAt: localHourKey(cellEnd),
      granularity: range === "today" ? "hour" : "four-hours",
      ...value,
    });
  }
  const maximum = Math.max(...cells.map((item) => item.total), 1);
  return cells.map((item) => ({
    ...item,
    level: item.total === 0 ? 0 : Math.max(1, Math.ceil((item.total / maximum) * 4)),
  }));
}

export function activityHourDate(value: string): Date {
  return parseLocalHour(value);
}

export function buildActivityDays(points: DailyUsagePoint[], range: RangeKey, referenceDate = localDateKey(new Date())): ActivityDay[] {
  const values = aggregateDaily(points);
  const earliest = [...values.keys()].sort()[0];
  const dayCount = range === "all" && earliest ? daysBetween(earliest, referenceDate) + 1 : range === "all" ? 1 : RANGE_DAYS[range];
  const startDate = range === "all" && earliest ? earliest : addDays(referenceDate, -dayCount + 1);
  const days = Array.from({ length: dayCount }, (_, index) => {
    const date = addDays(startDate, index);
    const value = values.get(date) ?? { codex: 0, claude: 0, total: 0 };
    return { date, ...value };
  });
  const maximum = Math.max(...days.map((item) => item.total), 1);
  return days.map((item) => ({
    ...item,
    level: item.total === 0 ? 0 : Math.max(1, Math.ceil((item.total / maximum) * 4)),
  }));
}

function sumBucket(days: ActivityDay[], granularity: TrendBucket["granularity"]): TrendBucket {
  return days.reduce<TrendBucket>((bucket, day) => ({
    ...bucket,
    codex: bucket.codex + day.codex,
    claude: bucket.claude + day.claude,
    total: bucket.total + day.total,
    endDate: day.date,
  }), {
    startDate: days[0].date,
    endDate: days.at(-1)?.date ?? days[0].date,
    granularity,
    codex: 0,
    claude: 0,
    total: 0,
  });
}

function groupByCalendarPeriod(days: ActivityDay[], granularity: "month" | "quarter"): TrendBucket[] {
  const groups = new Map<string, ActivityDay[]>();
  for (const day of days) {
    const month = Number(day.date.slice(5, 7));
    const key = granularity === "month" ? day.date.slice(0, 7) : `${day.date.slice(0, 4)}-Q${Math.ceil(month / 3)}`;
    groups.set(key, [...(groups.get(key) ?? []), day]);
  }
  return [...groups.values()].map((group) => sumBucket(group, granularity));
}

export function buildTrendBuckets(points: DailyUsagePoint[], range: RangeKey, referenceDate = localDateKey(new Date())): TrendBucket[] {
  const days = buildActivityDays(points, range, referenceDate);
  const allTimeGranularity = days.length <= 45 ? "day" : days.length <= 180 ? "week" : days.length <= 730 ? "month" : "quarter";
  const granularity: TrendBucket["granularity"] = range === "90d" ? "week" : range === "year" ? "month" : range === "all" ? allTimeGranularity : "day";
  if (granularity === "day") return days.map((day) => sumBucket([day], "day"));
  if (granularity === "month" || granularity === "quarter") return groupByCalendarPeriod(days, granularity);
  return Array.from({ length: Math.ceil(days.length / 7) }, (_, index) => sumBucket(days.slice(index * 7, index * 7 + 7), "week"));
}

export function heatmapLayout(dayCount: number, linear = false): HeatmapLayout {
  const weekLayout = !linear && dayCount > 30;
  const columns = weekLayout ? Math.ceil(dayCount / 7) : Math.max(1, dayCount);
  return { columns, rows: weekLayout ? 7 : 1, weekLayout, dense: columns > 35 };
}
