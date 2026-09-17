import { describe, expect, it } from "vitest";
import {
  SIDE_NOTCH_CORNER_RADIUS,
  SIDE_NOTCH_CURL_RADIUS,
  SIDE_NOTCH_DEPTH,
  sideNotchPath,
  sideNotchTransform,
} from "./sideNotchShape";

function arcs(path: string): { sweep: string; end: string }[] {
  return [...path.matchAll(/A ([\d.]+) [\d.]+ 0 0 (\d) ([\d.]+) ([\d.]+)/g)].map((match) => ({
    sweep: match[2],
    end: `${match[3]},${match[4]}`,
  }));
}

describe("side notch shape", () => {
  it("keeps codenotch's flare and corner ratios against the body depth", () => {
    // 1 : 0.553 : 0.423 in the reference. The ratios are the shape; the scale
    // is ours. Drifting one of the three is what turns the outline back into a
    // slab with two nicks in it.
    // Whole-pixel radii cannot land on the ratios exactly at this scale, so
    // the bound is relative: 2% is indistinguishable by eye, and nowhere near
    // the 0.35 that made the outline read as a slab with two nicks in it.
    expect(Math.abs(SIDE_NOTCH_CURL_RADIUS / SIDE_NOTCH_DEPTH - 0.553)).toBeLessThan(0.02);
    expect(Math.abs(SIDE_NOTCH_CORNER_RADIUS / SIDE_NOTCH_DEPTH - 0.423)).toBeLessThan(0.02);
    // The flare has to fit beside the corner or it gets clamped away.
    expect(SIDE_NOTCH_CURL_RADIUS).toBeLessThanOrEqual(
      SIDE_NOTCH_DEPTH - SIDE_NOTCH_CORNER_RADIUS,
    );
  });

  it("flares out to the bezel at both ends and rounds only the free side", () => {
    const path = sideNotchPath({ length: 223 });
    expect(path.startsWith(`M ${SIDE_NOTCH_DEPTH} 0`)).toBe(true);
    // Flares curve away from the body (sweep 1); the two body corners curve
    // into it (sweep 0). Flipping either one turns the notch into a capsule
    // that floats off the edge.
    expect(arcs(path).map((arc) => arc.sweep)).toEqual(["1", "0", "0", "1"]);
    expect(arcs(path)[0].end).toBe("19,24");
    expect(arcs(path)[3].end).toBe("43,223");
    expect(path.endsWith("Z")).toBe(true);
  });

  it("keeps the corner when the flare is as wide as the body", () => {
    // A 30pt-deep notch: the corner is claimed first out of half the depth, so
    // the flare gives way instead of squaring the body off.
    const path = sideNotchPath({ depth: 30, length: 300, cornerRadius: 18 });
    const radii = [...path.matchAll(/A ([\d.]+) /g)].map((match) => Number(match[1]));
    expect(radii).toEqual([15, 15, 15, 15]);
  });

  it("meets the bezel square when there is no room to flare", () => {
    const path = sideNotchPath({ depth: 68, length: 300, curlRadius: 0, cornerRadius: 18 });
    expect(arcs(path).map((arc) => arc.sweep)).toEqual(["0", "0"]);
    expect(path.startsWith("M 68 0 L 18 0")).toBe(true);
  });

  it("mirrors onto the left edge rather than writing a second outline", () => {
    expect(sideNotchTransform("left", SIDE_NOTCH_DEPTH)).toBe("translate(43 0) scale(-1 1)");
    expect(sideNotchTransform("right", SIDE_NOTCH_DEPTH)).toBeUndefined();
  });
});
