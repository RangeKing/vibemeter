// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { I18nextProvider } from "react-i18next";
import i18n from "../i18n";
import type { AppSettings, DiagnosticRetentionStatus, ProjectControl } from "../types";
import { DiagnosticRetentionControl, SettingsPage } from "./SettingsPage";

const { apiMocks, autostartMocks } = vi.hoisted(() => ({
  apiMocks: {
    settings: vi.fn(),
    projects: vi.fn(),
    sources: vi.fn(),
    liveSnapshot: vi.fn(),
    diagnosticRetention: vi.fn(),
    setSetting: vi.fn(),
    createProjectGroup: vi.fn(),
    refreshIndex: vi.fn(),
    indexStatus: vi.fn(),
  },
  autostartMocks: {
    isEnabled: vi.fn(),
    enable: vi.fn(),
    disable: vi.fn(),
  },
}));

vi.mock("../lib/api", () => ({ api: apiMocks }));
vi.mock("@tauri-apps/plugin-autostart", () => autostartMocks);

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

describe("Data page Agent display settings", () => {
  const settings: AppSettings = {
    locale: "zh-CN",
    theme: "system",
    onboardingComplete: "true",
    iaMigrationTipSeen: "false",
    credentialsAllowed: "false",
    cursorDashboardUsage: "false",
    useSystemProxy: "false",
    launchAtLogin: "false",
    gitReadAllowed: "false",
    vctiPromptStructure: "true",
    retentionDays: "365",
    liveHooksEnabled: "true",
    notchEnabled: "true",
    menuBarEnabled: "true",
    dataPageAgents: "auto",
  };

  beforeEach(() => {
    apiMocks.settings.mockResolvedValue(settings);
    apiMocks.projects.mockResolvedValue([]);
    apiMocks.sources.mockResolvedValue([
      { agent: "codex", available: true, selected: true, capabilityLevel: "full", liveCapability: "exact", parserVersion: "test", sessionCount: 1, status: "ready", warningCount: 0, pathLabel: "" },
      { agent: "grok-build", available: true, selected: true, capabilityLevel: "full", liveCapability: "exact", parserVersion: "test", sessionCount: 0, status: "ready", warningCount: 0, pathLabel: "" },
      { agent: "zcode", available: false, selected: false, capabilityLevel: "full", liveCapability: "exact", parserVersion: "test", sessionCount: 0, status: "not-found", warningCount: 0, pathLabel: "" },
    ]);
    apiMocks.liveSnapshot.mockResolvedValue({ hookStatus: { state: "ready", providers: [] } });
    apiMocks.diagnosticRetention.mockResolvedValue({ state: "disabled", enabled: false, storageLocation: "", retainedEnvelopes: 0 });
    apiMocks.setSetting.mockResolvedValue(undefined);
    apiMocks.createProjectGroup.mockResolvedValue("group-1");
    apiMocks.refreshIndex.mockResolvedValue(true);
    apiMocks.indexStatus.mockResolvedValue({ running: false, finishedAt: "forced-pass" });
    autostartMocks.isEnabled.mockResolvedValue(false);
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("keeps automatic detection on by default and saves a custom display list", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <I18nextProvider i18n={i18n}>
        <QueryClientProvider client={queryClient}>
          <SettingsPage locale="zh-CN" />
        </QueryClientProvider>
      </I18nextProvider>,
    );

    const automatic = await screen.findByRole("switch", { name: "自动显示已检测的 Agent" });
    expect(automatic.getAttribute("aria-checked")).toBe("true");
    expect((screen.getByRole("checkbox", { name: "zcode" }) as HTMLInputElement).disabled).toBe(true);

    fireEvent.click(automatic);
    expect(apiMocks.setSetting).toHaveBeenCalledWith("dataPageAgents", '["codex","grok-build"]');
    await waitFor(() => expect((screen.getByRole("checkbox", { name: "grok-build" }) as HTMLInputElement).disabled).toBe(false));

    fireEvent.click(screen.getByRole("checkbox", { name: "grok-build" }));
    expect(apiMocks.setSetting).toHaveBeenLastCalledWith("dataPageAgents", '["codex"]');
  });

  it("allows cancelling a detected Agent directly from automatic display mode", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <I18nextProvider i18n={i18n}>
        <QueryClientProvider client={queryClient}>
          <SettingsPage locale="zh-CN" />
        </QueryClientProvider>
      </I18nextProvider>,
    );

    const automatic = await screen.findByRole("switch", { name: "自动显示已检测的 Agent" });
    expect(automatic.getAttribute("aria-checked")).toBe("true");
    const codex = screen.getByRole("checkbox", { name: "codex" }) as HTMLInputElement;
    expect(codex.disabled).toBe(false);

    fireEvent.click(codex);

    expect(apiMocks.setSetting).toHaveBeenCalledWith("dataPageAgents", '["grok-build"]');
    expect(automatic.getAttribute("aria-checked")).toBe("false");
  });

  it("supports contiguous shift selection and non-contiguous command selection for projects", async () => {
    const project = (projectHash: string, projectLabel: string): ProjectControl => ({
      projectHash,
      projectLabel,
      sessionCount: 1,
      excluded: false,
      localPath: `/Users/test/${projectLabel.toLowerCase()}`,
    });
    apiMocks.projects.mockResolvedValue([
      project("project-a", "Project A"),
      project("project-b", "Project B"),
      project("project-c", "Project C"),
      project("project-d", "Project D"),
    ]);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <I18nextProvider i18n={i18n}>
        <QueryClientProvider client={queryClient}>
          <SettingsPage locale="zh-CN" />
        </QueryClientProvider>
      </I18nextProvider>,
    );

    const first = await screen.findByRole("checkbox", { name: "Project A" });
    const second = screen.getByRole("checkbox", { name: "Project B" });
    const third = screen.getByRole("checkbox", { name: "Project C" });
    const fourth = screen.getByRole("checkbox", { name: "Project D" });

    fireEvent.click(first);
    fireEvent.pointerDown(third, { shiftKey: true });
    fireEvent.click(third);
    expect((first as HTMLInputElement).checked).toBe(true);
    expect((second as HTMLInputElement).checked).toBe(true);
    expect((third as HTMLInputElement).checked).toBe(true);
    expect((fourth as HTMLInputElement).checked).toBe(false);

    fireEvent.pointerDown(fourth, { metaKey: true });
    fireEvent.click(fourth, { metaKey: true });
    expect((fourth as HTMLInputElement).checked).toBe(true);

    fireEvent.pointerDown(second, { metaKey: true });
    fireEvent.click(second, { metaKey: true });
    expect((first as HTMLInputElement).checked).toBe(true);
    expect((second as HTMLInputElement).checked).toBe(false);
    expect((third as HTMLInputElement).checked).toBe(true);
    expect((fourth as HTMLInputElement).checked).toBe(true);
  });

  it("opens an in-app project group dialog and creates the group", async () => {
    const project = (projectHash: string, projectLabel: string): ProjectControl => ({
      projectHash,
      projectLabel,
      sessionCount: 1,
      excluded: false,
      localPath: `/Users/test/${projectLabel.toLowerCase()}`,
    });
    apiMocks.projects.mockResolvedValue([
      project("project-a", "Project A"),
      project("project-b", "Project B"),
    ]);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <I18nextProvider i18n={i18n}>
        <QueryClientProvider client={queryClient}>
          <SettingsPage locale="zh-CN" />
        </QueryClientProvider>
      </I18nextProvider>,
    );

    fireEvent.click(await screen.findByRole("checkbox", { name: "Project A" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Project B" }));
    fireEvent.click(screen.getByRole("button", { name: "创建项目组（已选 2 个）" }));

    const dialog = screen.getByRole("dialog", { name: "请输入项目组名称" });
    const name = screen.getByRole("textbox", { name: "请输入项目组名称" });
    fireEvent.change(name, { target: { value: "Shared workspace" } });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => expect(apiMocks.createProjectGroup).toHaveBeenCalledWith("Shared workspace", ["project-a", "project-b"]));
    expect(dialog.isConnected).toBe(false);
  });
});
