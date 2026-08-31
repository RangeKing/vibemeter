import { describe, expect, it } from "vitest";
import type { DelegationEdge, DelegationNode } from "../../types";
import { buildDelegationGraphOption, filterDelegationTrace } from "./delegationGraphModel";

const nodes: DelegationNode[] = [
  { id: "root", kind: "root-agent", agent: "codex", safeLabel: "Codex · root", status: "running", evidenceLevel: "observed", sourceCoverage: "exact", confidence: 1 },
  { id: "child-a", kind: "subagent", agent: "codex", safeLabel: "Codex · child-a", status: "waiting", evidenceLevel: "observed", sourceCoverage: "exact", confidence: .95 },
  { id: "child-b", kind: "subagent", agent: "codex", safeLabel: "Codex · child-b", status: "running", evidenceLevel: "inferred", sourceCoverage: "partial", confidence: .45 },
];
const edges: DelegationEdge[] = [
  { id: "spawn", from: "root", to: "child-a", relationType: "spawn", status: "waiting", confidence: .95, evidenceLevel: "observed", sourceCoverage: "exact", algorithmVersion: "delegation-trace-1.0.0", evidenceIds: ["a"] },
  { id: "handoff", from: "root", to: "child-b", relationType: "handoff", status: "running", confidence: .45, evidenceLevel: "inferred", sourceCoverage: "partial", algorithmVersion: "delegation-trace-1.0.0", evidenceIds: ["b"] },
];
const colors = {
  text: "#111",
  textSecondary: "#555",
  hairline: "#ddd",
  paper: "#fff",
  active: "#00f",
  waiting: "#fa0",
  failed: "#f00",
  completed: "#0a0",
  unknown: "#999",
  relation: { spawn: "#07f", handoff: "#a60" },
};

describe("delegation graph model", () => {
  it("filters by relationship and status while retaining endpoints", () => {
    expect(filterDelegationTrace({ nodes, edges }, { agent: "all", relationType: "handoff", status: "all" })).toEqual({
      nodes: [nodes[0], nodes[2]],
      edges: [edges[1]],
    });
    expect(filterDelegationTrace({ nodes, edges }, { agent: "all", relationType: "all", status: "waiting" })).toEqual({
      nodes: [nodes[0], nodes[1]],
      edges: [edges[0]],
    });
  });

  it("lays out root and parallel children deterministically and distinguishes inferred edges", () => {
    const option = buildDelegationGraphOption({
      nodes,
      edges,
      colors,
      selection: { type: "edge", id: "handoff" },
      relationLabel: (value) => value,
      statusLabel: (value) => value,
      restoreLabel: "fit",
    }) as { series: Array<{ data: Array<Record<string, unknown>>; links: Array<Record<string, unknown>>; roam: boolean }> };
    const series = option.series[0]!;
    expect(series.roam).toBe(true);
    expect(series.data.find((node) => node.id === "root")?.symbol).toBe("diamond");
    expect(series.data.filter((node) => node.id !== "root").map((node) => node.x)).toEqual([220, 220]);
    const inferred = series.links.find((edge) => edge.id === "handoff")!;
    expect(inferred.lineStyle).toMatchObject({ type: "dashed", width: 4, opacity: .58 });
  });

  it("takes all visual colors from the active theme contract", () => {
    const option = buildDelegationGraphOption({
      nodes,
      edges,
      colors: { ...colors, paper: "#101010", text: "#fefefe" },
      relationLabel: (value) => value,
      statusLabel: (value) => value,
      restoreLabel: "fit",
    }) as { series: Array<{ data: Array<{ itemStyle: { color: string }; label: { color: string } }> }> };
    expect(option.series[0]!.data[0]!.itemStyle.color).toBe("#101010");
    expect(option.series[0]!.data[0]!.label.color).toBe("#fefefe");
  });
});
