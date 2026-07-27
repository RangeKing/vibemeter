// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { I18nextProvider } from "react-i18next";
import i18n from "../i18n";
import { useUiStore } from "../store";
import type { ShareRenderRequest } from "../types";
import { ShareStudioPage } from "./ShareStudioPage";

const { exportShare, previewShare, saveDialog, sessionsApi } = vi.hoisted(() => ({
  exportShare: vi.fn(),
  previewShare: vi.fn(async (request: ShareRenderRequest) => ({
    svg: `<svg data-range="${request.range}" />`,
    width: 2160,
    height: 2880,
    findings: [],
    canExport: true,
    modelHash: request.range,
  })),
  saveDialog: vi.fn(async () => "/tmp/vibemeter-share.png"),
  sessionsApi: vi.fn(async () => ({
    items: [
      { id: "session-one", title: "修复 Notch", model: "gpt-5.6-sol", agent: "codex", projectLabel: "vibemeter" },
      { id: "session-two", title: "完善 VCTI", model: "claude-opus", agent: "claude-code", projectLabel: "vibemeter" },
    ],
    total: 2,
  })),
}));

vi.mock("../lib/api", () => ({
  api: {
    sessions: sessionsApi,
    previewShare,
    exportShare,
    renderSharePng: vi.fn(),
  },
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({ save: saveDialog }));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({ writeImage: vi.fn() }));

function renderShare() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  render(
    <I18nextProvider i18n={i18n}>
      <QueryClientProvider client={queryClient}>
        <ShareStudioPage locale="zh-CN" />
      </QueryClientProvider>
    </I18nextProvider>,
  );
}

describe("ShareStudioPage range controls", () => {
  beforeAll(async () => {
    await i18n.changeLanguage("zh-CN");
  });

  beforeEach(() => {
    exportShare.mockClear();
    previewShare.mockClear();
    saveDialog.mockClear();
    sessionsApi.mockClear();
    useUiStore.setState({ range: "year" });
  });

  afterEach(cleanup);

  it("uses the shared Data range for preview and updates it from the same picker", async () => {
    renderShare();

    await waitFor(() => expect(previewShare).toHaveBeenCalledWith(expect.objectContaining({ range: "year" })));
    expect(screen.getByRole("button", { name: "一年" }).classList.contains("active")).toBe(true);
    expect(screen.getByRole("button", { name: "一年" }).getAttribute("aria-pressed")).toBe("true");

    fireEvent.click(screen.getByRole("button", { name: "今天" }));

    await waitFor(() => expect(previewShare).toHaveBeenCalledWith(expect.objectContaining({ range: "today" })));
    expect(useUiStore.getState().range).toBe("today");
  });

  it("only exposes the four data-share templates", async () => {
    renderShare();

    expect(screen.getAllByRole("button", { name: /^D[1-4]/ })).toHaveLength(4);
    expect(screen.queryByRole("button", { name: /每周回顾/ })).toBeNull();
  });

  it("exports with the currently selected shared range", async () => {
    renderShare();

    const exportButton = await screen.findByRole("button", { name: "导出 PNG" });
    await waitFor(() => expect((exportButton as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(exportButton);

    await waitFor(() => expect(exportShare).toHaveBeenCalledWith(
      expect.objectContaining({ range: "year" }),
      "png",
      "/tmp/vibemeter-share.png",
    ));
  });

  it("shows the session picker beside the selected recap template and previews the chosen session", async () => {
    renderShare();

    fireEvent.click(screen.getByRole("button", { name: /^D4/ }));
    const picker = await screen.findByRole("combobox", { name: "选择单次会话" });
    expect(screen.getByText("预览和导出都会使用这里选中的会话。")).toBeTruthy();
    await screen.findByRole("option", { name: "完善 VCTI · vibemeter" });
    await waitFor(() => expect((picker as HTMLSelectElement).disabled).toBe(false));

    fireEvent.change(picker, { target: { value: "session-two" } });

    await waitFor(() => expect(previewShare).toHaveBeenCalledWith(
      expect.objectContaining({ templateId: "session-recap", sessionId: "session-two" }),
    ));
  });
});
