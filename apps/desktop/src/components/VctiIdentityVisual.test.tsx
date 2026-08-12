// @vitest-environment jsdom

import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { VctiIdentityVisual as IdentityVisual } from "../types";
import { VctiIdentityVisual } from "./VctiIdentityVisual";

describe("VctiIdentityVisual", () => {
  it("renders the exact shared path geometry and version", () => {
    const visual: IdentityVisual = {
      algorithmVersion: "1.6.0",
      version: "1.0.0",
      range: "90d",
      available: true,
      inputs: [{ id: "dimensions", available: true }],
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
    };

    render(<VctiIdentityVisual visual={visual} type="SPEC" guild="start" label="规划型" />);

    const image = screen.getByRole("img", { name: "规划型" });
    expect(image.getAttribute("data-vcti-visual-version")).toBe("1.0.0");
    expect(image.querySelector("path")?.getAttribute("d")).toBe("M10,10Q50,0 90,10Z");
    expect(image.textContent).toContain("SPEC");
  });
});
