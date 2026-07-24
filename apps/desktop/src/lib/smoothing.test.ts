import { describe, expect, it } from "vitest";
import { savitzkyGolaySmooth } from "./smoothing";

describe("Savitzky–Golay usage smoothing", () => {
  it("preserves a constant series with mirrored boundaries", () => {
    for (const value of savitzkyGolaySmooth([5, 5, 5, 5, 5, 5, 5])) {
      expect(value).toBeCloseTo(5, 10);
    }
  });

  it("preserves a quadratic at the center of a seven-point window", () => {
    const smoothed = savitzkyGolaySmooth([0, 1, 4, 9, 16, 25, 36]);
    expect(smoothed[3]).toBeCloseTo(9, 10);
  });

  it("uses the five-point kernel for shorter eligible series", () => {
    const smoothed = savitzkyGolaySmooth([0, 1, 4, 9, 16]);
    expect(smoothed[2]).toBeCloseTo(4, 10);
  });

  it("keeps very short series unchanged and never returns negative usage", () => {
    expect(savitzkyGolaySmooth([1, 2, 3, 4])).toEqual([1, 2, 3, 4]);
    expect(savitzkyGolaySmooth([0, 0, 0, 21, 0, 0, 0]).every((value) => value >= 0)).toBe(true);
  });
});
