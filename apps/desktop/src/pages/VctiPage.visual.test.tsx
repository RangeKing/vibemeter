// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { I18nextProvider } from "react-i18next";
import i18n from "../i18n";
import { useUiStore } from "../store";
import type { VctiProfile } from "../types";
import { VctiPage } from "./VctiPage";

const { insights, phraseCloud, vctiProfile } = vi.hoisted(() => ({
  insights: vi.fn(),
  phraseCloud: vi.fn(),
  vctiProfile: vi.fn(),
}));

vi.mock("../lib/api", () => ({
  api: { insights, phraseCloud, vctiProfile },
}));

vi.mock("../components/RangePicker", () => ({ RangePicker: () => null }));
vi.mock("../components/BehaviorStreams", () => ({ BehaviorStreams: () => null }));
vi.mock("../components/CatchphraseClouds", () => ({ CatchphraseClouds: () => null }));

const profile = {
  range: "90d",
  periodStart: "2026-05-16",
  periodEnd: "2026-08-13",
  status: "stable",
  temporary: false,
  primaryType: "SPEC",
  secondaryType: undefined,
  guild: "start",
  confidence: 82,
  confidenceLabel: "high",
  sessionCount: 24,
  activeDays: 12,
  scores: [],
  badges: [],
  evidence: [],
  trend: [],
  missingCapabilities: [],
  behavior: {
    sessions: 24,
    structureCoverage: 1,
    structureCapableSessions: 24,
    lifecycleCoverage: 1,
    lifecycleCapableSessions: 24,
    orchestrationCoverage: 1,
    orchestrationCapableSessions: 24,
    toolResultCoverage: 1,
    toolResultCapableSessions: 24,
    processControlCoverage: 1,
    processControlCapableSessions: 24,
  },
  identityEvidence: {
    rhythm: {
      workPeriods: [
        { id: "morning", sessions: 8, share: 2 / 3 },
        { id: "afternoon", sessions: 4, share: 1 / 3 },
      ],
      workPeriodsAvailable: true,
      activeDays: { available: true, value: 12 },
      sessionsPerDay: { available: true, value: 2 },
    },
    collaboration: {
      subagentStarts: { available: true, value: 7 },
      parallelBatches: { available: true, value: 2 },
    },
    detailDiversity: {
      toolCategories: { available: true, value: 5 },
      explicitSkills: { available: true, value: 3 },
    },
    processVariation: {
      errors: { available: true, value: 0 },
      retries: { available: true, value: 2 },
      rollbacks: { available: false },
    },
  },
} as unknown as VctiProfile;

function renderPage() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  return render(
    <I18nextProvider i18n={i18n}>
      <QueryClientProvider client={queryClient}>
        <VctiPage locale="zh-CN" />
      </QueryClientProvider>
    </I18nextProvider>,
  );
}

describe("VctiPage identity art", () => {
  beforeAll(async () => {
    await i18n.changeLanguage("zh-CN");
  });

  afterEach(cleanup);

  it("keeps the original VCTI character as the primary image", async () => {
    useUiStore.setState({ range: "90d" });
    vctiProfile.mockResolvedValue(profile);
    insights.mockResolvedValue({ items: [] });
    phraseCloud.mockRejectedValue(new Error("not needed"));

    const { container } = renderPage();

    const character = await screen.findByRole("img", { name: "开工判官" });
    expect(character.classList.contains("vcti-avatar")).toBe(true);
    expect(character.querySelector(".vcti-avatar-art")).toBeTruthy();
    expect(container.querySelector("[data-vcti-visual-version]")).toBeNull();
  });

  it("shows all four identity evidence summaries before opening the detail drawer", async () => {
    useUiStore.setState({ range: "90d" });
    vctiProfile.mockResolvedValue(profile);
    insights.mockResolvedValue({ items: [] });
    phraseCloud.mockRejectedValue(new Error("not needed"));

    renderPage();

    expect(await screen.findByRole("region", { name: "人格依据" })).toBeTruthy();
    expect(screen.getByText("工作节奏")).toBeTruthy();
    expect(screen.getByText("协作方式")).toBeTruthy();
    expect(screen.getByText("工具与 Skill")).toBeTruthy();
    expect(screen.getByText("过程记录")).toBeTruthy();
    expect(screen.getByText(/错误 0/)).toBeTruthy();
    expect(screen.getByText(/回滚 未记录/)).toBeTruthy();
    expect(screen.queryByRole("dialog", { name: "为什么是这个人格" })).toBeNull();
  });

  it("treats an available metric without a value as not recorded", async () => {
    useUiStore.setState({ range: "90d" });
    vctiProfile.mockResolvedValue({
      ...profile,
      identityEvidence: {
        ...profile.identityEvidence,
        processVariation: {
          ...profile.identityEvidence.processVariation,
          errors: { available: true },
        },
      },
    });
    insights.mockResolvedValue({ items: [] });
    phraseCloud.mockRejectedValue(new Error("not needed"));

    renderPage();

    expect(await screen.findByText(/错误 未记录/)).toBeTruthy();
  });

  it("keeps the collecting state focused on progress instead of a finished evidence summary", async () => {
    useUiStore.setState({ range: "today" });
    vctiProfile.mockResolvedValue({
      ...profile,
      status: "collecting",
      primaryType: undefined,
      confidence: 4,
    });
    insights.mockResolvedValue({ items: [] });
    phraseCloud.mockRejectedValue(new Error("not needed"));

    renderPage();

    expect(await screen.findByText("还在了解你")).toBeTruthy();
    expect(screen.queryByRole("region", { name: "人格依据" })).toBeNull();
  });
});
