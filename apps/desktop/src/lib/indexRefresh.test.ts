import { describe, expect, it, vi } from "vitest";
import { refreshHistoryIndex } from "./indexRefresh";

describe("history evidence refresh", () => {
  it("waits for a running index, starts a permission-aware pass, and refreshes session data only after completion", async () => {
    const start = vi.fn()
      .mockResolvedValueOnce(false)
      .mockResolvedValueOnce(true);
    const statuses = [
      { running: true, finishedAt: undefined },
      { running: false, finishedAt: "first-pass" },
      { running: true, finishedAt: undefined },
      { running: false, finishedAt: "forced-pass" },
    ];
    const status = vi.fn(async () => statuses.shift() ?? { running: false, finishedAt: "forced-pass" });
    const completed = vi.fn();

    await refreshHistoryIndex({ start, status, completed, pollIntervalMs: 0 });

    expect(start).toHaveBeenCalledTimes(2);
    expect(start).toHaveBeenNthCalledWith(1, false);
    expect(start).toHaveBeenNthCalledWith(2, false);
    expect(completed).toHaveBeenCalledOnce();
    expect(status).toHaveBeenCalledTimes(4);
  });
});
