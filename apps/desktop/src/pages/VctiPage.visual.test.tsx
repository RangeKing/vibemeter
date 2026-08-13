// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen } from "@testing-library/react";
import { act } from "react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { I18nextProvider } from "react-i18next";
import i18n from "../i18n";
import { useUiStore } from "../store";
import type { VctiProfile } from "../types";
import { VctiPage } from "./VctiPage";
import { VctiArtField } from "../components/VctiArtPortrait";

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
  identityVisual: {
    algorithmVersion: "1.6.0",
    version: "2.1.0",
    range: "90d",
    available: true,
    contours: [{ d: "M 10 50 Q 50 10 90 50 Q 50 90 10 50 Z", strokeWidth: 0.8, opacity: 0.5 }],
    rhythm: { available: true, phase: 0.25, density: 0.6, paths: [{ d: "M 4 42 Q 50 18 96 58", strokeWidth: 0.7, opacity: 0.4 }] },
    collaboration: { available: true, branchIntensity: 0.5, parallelIntensity: 0.25, paths: [{ d: "M 50 50 Q 72 35 92 20", strokeWidth: 0.7, opacity: 0.5 }] },
    detail: { available: true, toolIntensity: 0.5, skillIntensity: 0.4, toolMarks: [{ cx: 12, cy: 34, radius: 1, opacity: 0.7 }], skillMarks: [{ cx: 86, cy: 62, radius: 1.5, opacity: 0.8 }] },
    process: { available: true, errorIntensity: 0.2, retryIntensity: 0.3, rollbackIntensity: 0.1, paths: [{ d: "M 18 70 Q 50 55 82 70", strokeWidth: 0.8, opacity: 0.6 }] },
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
    expect(container.querySelector('[data-vcti-visual-version="2.1.0"]')).toBeTruthy();
    expect(container.querySelectorAll(".vcti-art-contours path")).toHaveLength(1);
    expect(container.querySelectorAll(".vcti-art-rhythm path")).toHaveLength(1);
    expect(container.querySelectorAll(".vcti-art-branches path")).toHaveLength(1);
    expect(container.querySelectorAll('.vcti-art-tools [data-mark="tool"]')).toHaveLength(1);
    expect(container.querySelectorAll('.vcti-art-skills [data-mark="skill"]')).toHaveLength(1);
    expect(container.querySelectorAll(".vcti-art-process path")).toHaveLength(1);
  });

  it("makes one behavior terrain the primary composition and keeps text as a compact legend", async () => {
    useUiStore.setState({ range: "90d" });
    vctiProfile.mockResolvedValue(profile);
    insights.mockResolvedValue({ items: [] });
    phraseCloud.mockRejectedValue(new Error("not needed"));

    const { container } = renderPage();

    expect(await screen.findByRole("figure", { name: /开工判官/ })).toBeTruthy();
    expect(container.querySelectorAll(".vcti-terrain > g")).toHaveLength(5);
    expect(screen.getByRole("list", { name: "人格依据" })).toBeTruthy();
    expect(screen.getByText("工作节奏")).toBeTruthy();
    expect(screen.getByText("协作方式")).toBeTruthy();
    expect(screen.getByText("工具与 Skill")).toBeTruthy();
    expect(screen.getByText("过程记录")).toBeTruthy();
    expect(screen.queryByText(/错误 0/)).toBeNull();
    expect(screen.queryByRole("dialog", { name: "为什么是这个人格" })).toBeNull();
  });

  it("places a channel-distinct art field across the whole identity card instead of behind the portrait", async () => {
    useUiStore.setState({ range: "90d" });
    vctiProfile.mockResolvedValue(profile);
    insights.mockResolvedValue({ items: [] });
    phraseCloud.mockRejectedValue(new Error("not needed"));

    const { container } = renderPage();
    await screen.findByRole("figure", { name: /开工判官/ });
    const reveal = container.querySelector(".vcti-reveal");
    const field = reveal?.querySelector(":scope > .vcti-art-field");
    const portrait = reveal?.querySelector(":scope > .vcti-art-portrait");
    expect(field).toBeTruthy();
    expect(portrait).toBeTruthy();
    expect(portrait?.querySelector(".vcti-art-field")).toBeNull();
    expect(field?.getAttribute("viewBox")).toBe("0 0 160 100");
    for (const channel of ["rhythm", "branches", "detail", "process"]) {
      expect(field?.querySelector(`[data-visual-channel="${channel}"]`)).toBeTruthy();
    }
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

    expect(await screen.findByText("过程记录")).toBeTruthy();
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
    expect(document.querySelector("[data-vcti-visual-version]")).toBeNull();
  });

  it("finishes the short generation motion and reduced motion starts at the same final state", async () => {
    vi.useFakeTimers();
    Object.defineProperty(window, "matchMedia", { configurable: true, value: vi.fn(() => ({ matches: false })) });
    const { container, unmount } = render(<VctiArtField visual={profile.identityVisual} type="SPEC" guild="start" />);
    expect(container.querySelector(".vcti-art-field")?.getAttribute("data-generating")).toBe("true");
    act(() => vi.advanceTimersByTime(900));
    expect(container.querySelector(".vcti-art-field")?.getAttribute("data-generating")).toBe("false");
    unmount();

    Object.defineProperty(window, "matchMedia", { configurable: true, value: vi.fn(() => ({ matches: true })) });
    const reduced = render(<VctiArtField visual={profile.identityVisual} type="SPEC" guild="start" />);
    expect(reduced.container.querySelector(".vcti-art-field")?.getAttribute("data-generating")).toBe("false");
    vi.useRealTimers();
  });
});
