import type { DailyUsagePoint } from "../types";
import { tokenTotal } from "./format";

export const MENU_ACTIVITY_DAYS = 15;

export type MenuActivityDay = {
  date: string;
  value: number;
  sessions: number;
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
  dayCount = MENU_ACTIVITY_DAYS,
): MenuActivityDay[] {
  const observed = new Map<string, Omit<MenuActivityDay, "date">>();
  for (const point of points) {
    const current = observed.get(point.date) ?? { value: 0, sessions: 0 };
    current.value += tokenTotal(point.usage);
    current.sessions += point.sessionCount;
    observed.set(point.date, current);
  }

  const count = Math.max(1, dayCount);
  const cursor = new Date(referenceTime);
  cursor.setHours(0, 0, 0, 0);
  cursor.setDate(cursor.getDate() - count + 1);
  return Array.from({ length: count }, () => {
    const date = localDateKey(cursor);
    const current = observed.get(date) ?? { value: 0, sessions: 0 };
    cursor.setDate(cursor.getDate() + 1);
    return { date, ...current };
  });
}
