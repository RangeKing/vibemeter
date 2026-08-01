import type { DailyUsagePoint, RangeKey } from "../types";
import { tokenTotal } from "./format";

export const MENU_ACTIVITY_MAX_BARS = 15;

export type MenuActivityBucket = {
  key: string;
  startDate: string;
  endDate: string;
  value: number;
  sessions: number;
};

const RANGE_DAYS: Record<RangeKey, number> = {
  today: 1,
  "7d": 7,
  "30d": 30,
  "90d": 90,
  "180d": 180,
  year: 365,
  all: 3650,
};

function localDateKey(date: Date): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

export function buildMenuActivity(
  points: DailyUsagePoint[],
  referenceTime: Date,
  range: RangeKey,
): MenuActivityBucket[] {
  const observed = new Map<string, { value: number; sessions: number }>();
  for (const point of points) {
    const current = observed.get(point.date) ?? { value: 0, sessions: 0 };
    current.value += tokenTotal(point.usage);
    current.sessions += point.sessionCount;
    observed.set(point.date, current);
  }

  const count = RANGE_DAYS[range];
  const cursor = new Date(referenceTime);
  cursor.setHours(0, 0, 0, 0);
  cursor.setDate(cursor.getDate() - count + 1);
  const days = Array.from({ length: count }, () => {
    const date = localDateKey(cursor);
    const current = observed.get(date) ?? { value: 0, sessions: 0 };
    cursor.setDate(cursor.getDate() + 1);
    return { date, ...current };
  });

  const bucketSize = Math.ceil(count / MENU_ACTIVITY_MAX_BARS);
  const buckets: MenuActivityBucket[] = [];
  for (let index = 0; index < days.length; index += bucketSize) {
    const group = days.slice(index, index + bucketSize);
    const startDate = group[0].date;
    const endDate = group.at(-1)?.date ?? startDate;
    buckets.push({
      key: `${startDate}:${endDate}`,
      startDate,
      endDate,
      value: group.reduce((total, day) => total + day.value, 0),
      sessions: group.reduce((total, day) => total + day.sessions, 0),
    });
  }
  return buckets;
}
