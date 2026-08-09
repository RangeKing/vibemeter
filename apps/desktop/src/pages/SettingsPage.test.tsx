// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { I18nextProvider } from "react-i18next";
import i18n from "../i18n";
import type { DiagnosticRetentionStatus } from "../types";
import { DiagnosticRetentionControl } from "./SettingsPage";

const active: DiagnosticRetentionStatus = {
  state: "active",
  enabled: true,
  startedAt: "2026-08-10T00:00:00Z",
  expiresAt: "2026-08-17T00:00:00Z",
  storageLocation: "/Users/test/Library/Application Support/com.vibemeter.desktop/vibemeter.sqlite",
  retainedEnvelopes: 3,
};

function renderControl(
  status: DiagnosticRetentionStatus | undefined,
  options: { hasError?: boolean; clearCount?: number | null } = {},
) {
  const onToggle = vi.fn();
  const onClear = vi.fn();
  render(
    <I18nextProvider i18n={i18n}>
      <DiagnosticRetentionControl
        status={status}
        locale="zh-CN"
        pending={false}
        loading={false}
        hasError={options.hasError ?? false}
        clearCount={options.clearCount ?? null}
        onToggle={onToggle}
        onClear={onClear}
      />
    </I18nextProvider>,
  );
  return { onToggle, onClear };
}

describe("diagnostic retention controls", () => {
  beforeAll(async () => {
    await i18n.changeLanguage("zh-CN");
  });

  afterEach(cleanup);

  it("shows the consent scope, location, window, count, and early-clear action", () => {
    const { onToggle, onClear } = renderControl(active);

    expect(screen.getByText(/未经裁剪的实时信封可能包含提示词、命令、代码、路径和工具输出/)).toBeTruthy();
    expect(screen.getByText(active.storageLocation)).toBeTruthy();
    expect(screen.getByText("3")).toBeTruthy();
    expect(screen.getAllByText(/2026/)).toHaveLength(2);

    fireEvent.click(screen.getByRole("switch", { name: "七天诊断模式" }));
    fireEvent.click(screen.getByRole("button", { name: "清除诊断数据" }));
    expect(onToggle).toHaveBeenCalledWith(false);
    expect(onClear).toHaveBeenCalledOnce();
  });

  it("reports secure-storage refusal and confirms completed clearing", () => {
    renderControl(
      { ...active, state: "unavailable", enabled: false, retainedEnvelopes: 0 },
      { hasError: true, clearCount: 3 },
    );

    expect(screen.getByRole("alert").textContent).toContain("不会降级为明文保存");
    expect(screen.getByRole("status").textContent).toContain("已清除 3 条诊断信封");
    expect(screen.getByText("安全存储不可用")).toBeTruthy();
    expect(screen.getByRole("button", { name: "清除诊断数据" })).toBeTruthy();
  });
});
