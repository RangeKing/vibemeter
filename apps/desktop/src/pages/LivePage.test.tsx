// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { I18nextProvider } from "react-i18next";
import i18n from "../i18n";
import type { LiveHistoryItem } from "../types";
import { HistoryList } from "./LivePage";

const historyItem: LiveHistoryItem = {
  id: "621",
  occurredAt: "2026-07-26T02:54:06Z",
  agent: "claude-code",
  projectLabel: "GlobalPhotovoltaic",
  status: "waiting",
  eventName: "PermissionRequest",
  sourceSessionId: "source-session",
  sessionId: "indexed-session",
};

function renderHistory(item: LiveHistoryItem, onOpenSession = vi.fn()) {
  render(
    <I18nextProvider i18n={i18n}>
      <HistoryList items={[item]} locale="zh-CN" onOpenSession={onOpenSession} />
    </I18nextProvider>,
  );
  return onOpenSession;
}

describe("Live history", () => {
  beforeAll(async () => {
    await i18n.changeLanguage("zh-CN");
  });

  afterEach(cleanup);

  it("shows a full local date and opens the matching indexed session", () => {
    const onOpenSession = renderHistory(historyItem);

    expect(screen.getByText(/2026年7月26日/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "返回源会话" }));

    expect(onOpenSession).toHaveBeenCalledWith("indexed-session");
  });

  it("keeps the jump action visible but disabled until indexing finishes", () => {
    renderHistory({ ...historyItem, sessionId: undefined });

    const button = screen.getByRole("button", { name: "返回源会话" }) as HTMLButtonElement;
    expect(button.disabled).toBe(true);
    expect(button.title).toBe("对应的本机会话尚未完成索引。");
  });
});
