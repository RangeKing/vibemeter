// @vitest-environment jsdom

import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { VctiIdentityVisual as IdentityVisual } from "../types";
import { VctiIdentityVisual } from "./VctiIdentityVisual";

describe("VctiIdentityVisual", () => {
  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("renders the exact shared path geometry and version", () => {
    const visual: IdentityVisual = {
      algorithmVersion: "1.6.0",
      version: "1.0.0",
      range: "90d",
      available: true,
      inputs: [{ id: "dimensions", available: true, status: "recorded" }],
      rhythm: {
        workPeriods: [],
        workPeriodsAvailable: true,
        activeDays: { value: 12, available: true },
        sessionsPerDay: { value: 1.4, available: true },
        phaseOffset: 0,
        contourCount: 7,
        contourSpacing: 4.5,
      },
      paths: [{ d: "M10,10Q50,0 90,10Z", strokeWidth: 1.25, opacity: 0.8 }],
      collaboration: {
        subagentStarts: { value: 1, available: true },
        parallelBatches: { value: 0, available: true },
        branchCount: 1,
        parallelSpread: 0,
      },
      branches: [{ d: "M50,50Q60,40 80,30", strokeWidth: 1, opacity: 0.6 }],
      detailDiversity: {
        toolCategories: { value: 3, available: true },
        explicitSkills: { value: 1, available: true },
        detailCount: 4,
      },
      details: [{ cx: 25, cy: 35, radius: 0.8, opacity: 0.7 }],
      processVariation: {
        errors: { value: 1, available: true },
        retries: { value: 0, available: true },
        rollbacks: { value: 0, available: true },
        variationCount: 1,
      },
      variations: [{ d: "M20,20Q25,18 30,20", strokeWidth: 1.15, opacity: 0.5 }],
    };

    render(<VctiIdentityVisual visual={visual} type="SPEC" guild="start" label="规划型" />);

    const image = screen.getByRole("img", { name: "规划型" });
    expect(image.getAttribute("data-vcti-visual-version")).toBe("1.0.0");
    expect(image.querySelector("path")?.getAttribute("d")).toBe("M10,10Q50,0 90,10Z");
    expect(image.querySelector(".vcti-identity-branches path")?.getAttribute("d")).toBe("M50,50Q60,40 80,30");
    expect(image.querySelector(".vcti-identity-details circle")?.getAttribute("cx")).toBe("25");
    expect(image.querySelector(".vcti-identity-variations path")?.getAttribute("d")).toBe("M20,20Q25,18 30,20");
    expect(image.textContent).toContain("SPEC");
  });

  it("animates once per result and cancels stale range transitions", () => {
    vi.useFakeTimers();
    vi.stubGlobal("matchMedia", vi.fn().mockReturnValue({ matches: false }));
    const visual: IdentityVisual = {
      algorithmVersion: "1.6.0",
      version: "1.0.0",
      range: "30d",
      available: true,
      inputs: [],
      rhythm: { workPeriods: [], workPeriodsAvailable: true, activeDays: { value: 2, available: true }, sessionsPerDay: { value: 1, available: true } },
      paths: [{ d: "M0,0Q5,5 10,0Z", strokeWidth: 1, opacity: 1 }],
      collaboration: { subagentStarts: { value: 0, available: true }, parallelBatches: { value: 0, available: true }, branchCount: 0, parallelSpread: 0 },
      branches: [],
      detailDiversity: { toolCategories: { value: 1, available: true }, explicitSkills: { value: 0, available: true }, detailCount: 1 },
      details: [],
      processVariation: { errors: { value: 0, available: true }, retries: { value: 0, available: true }, rollbacks: { value: 0, available: true }, variationCount: 0 },
      variations: [],
    };
    const { rerender, unmount } = render(<VctiIdentityVisual visual={visual} type="SPEC" guild="start" label="视觉" />);
    expect(screen.getByRole("img", { name: "视觉" }).getAttribute("data-generating")).toBe("true");
    rerender(<VctiIdentityVisual visual={{ ...visual, range: "90d" }} type="SPEC" guild="start" label="视觉" />);
    act(() => vi.advanceTimersByTime(850));
    expect(screen.getByRole("img", { name: "视觉" }).getAttribute("data-generating")).toBe("false");
    expect(() => unmount()).not.toThrow();
  });

  it("shows the static final state when reduced motion is requested", () => {
    vi.stubGlobal("matchMedia", vi.fn().mockReturnValue({ matches: true }));
    const visual = {
      algorithmVersion: "1.6.0", version: "1.0.0", range: "90d", available: true, inputs: [],
      rhythm: { workPeriods: [], workPeriodsAvailable: true, activeDays: { value: 1, available: true }, sessionsPerDay: { value: 1, available: true } },
      paths: [], collaboration: { subagentStarts: { value: 0, available: true }, parallelBatches: { value: 0, available: true } }, branches: [],
      detailDiversity: { toolCategories: { value: 0, available: true }, explicitSkills: { value: 0, available: true } }, details: [],
      processVariation: { errors: { value: 0, available: true }, retries: { value: 0, available: true }, rollbacks: { value: 0, available: true } }, variations: [],
    } satisfies IdentityVisual;
    render(<VctiIdentityVisual visual={visual} label="静态视觉" />);
    expect(screen.getByRole("img", { name: "静态视觉" }).getAttribute("data-generating")).toBe("false");
  });
});
