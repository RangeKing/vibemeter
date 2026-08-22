import type { IndexStatus } from "../types";

type RefreshHistoryIndexOptions = {
  start: (force: boolean) => Promise<boolean>;
  status: () => Promise<Pick<IndexStatus, "running" | "finishedAt">>;
  completed: () => void | Promise<void>;
  force?: boolean;
  pollIntervalMs?: number;
  maxPolls?: number;
};

function pause(milliseconds: number): Promise<void> {
  return new Promise((resolve) => globalThis.setTimeout(resolve, milliseconds));
}

async function waitUntilIdle(
  status: RefreshHistoryIndexOptions["status"],
  pollIntervalMs: number,
  maxPolls: number,
): Promise<void> {
  for (let poll = 0; poll < maxPolls; poll += 1) {
    if (!(await status()).running) return;
    await pause(pollIntervalMs);
  }
  throw new Error("history index refresh timed out");
}

export async function refreshHistoryIndex({
  start,
  status,
  completed,
  force = false,
  pollIntervalMs = 250,
  maxPolls = 480,
}: RefreshHistoryIndexOptions): Promise<void> {
  let started = await start(force);
  if (!started) {
    await waitUntilIdle(status, pollIntervalMs, maxPolls);
    started = await start(force);
  }
  if (!started) throw new Error("history index refresh could not start");
  await waitUntilIdle(status, pollIntervalMs, maxPolls);
  await completed();
}
