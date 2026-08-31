import type * as echarts from "echarts";
import type { DelegationEdge, DelegationNode, DelegationTraceResponse } from "../../types";

export type DelegationSelection =
  | { type: "node"; id: string }
  | { type: "edge"; id: string };

export type DelegationFilterValue = {
  agent: string;
  relationType: string;
  status: string;
};

export type DelegationGraphColors = {
  text: string;
  textSecondary: string;
  hairline: string;
  paper: string;
  active: string;
  waiting: string;
  failed: string;
  completed: string;
  unknown: string;
  relation: Record<string, string>;
};

export function filterDelegationTrace(
  trace: Pick<DelegationTraceResponse, "nodes" | "edges">,
  filters: DelegationFilterValue,
): { nodes: DelegationNode[]; edges: DelegationEdge[] } {
  const allowedNodes = new Set(
    trace.nodes
      .filter((node) => filters.agent === "all" || node.agent === filters.agent)
      .map((node) => node.id),
  );
  const edges = trace.edges.filter((edge) =>
    allowedNodes.has(edge.from)
    && allowedNodes.has(edge.to)
    && (filters.relationType === "all" || edge.relationType === filters.relationType)
    && (filters.status === "all" || edge.status === filters.status),
  );
  const connected = new Set(edges.flatMap((edge) => [edge.from, edge.to]));
  const nodes = trace.nodes.filter((node) =>
    allowedNodes.has(node.id)
    && (
      filters.status === "all"
      || node.status === filters.status
      || connected.has(node.id)
    )
    && (
      filters.relationType === "all"
      || connected.has(node.id)
      || node.kind === "root-agent"
    ),
  );
  return { nodes, edges };
}

function nodeDepths(nodes: DelegationNode[], edges: DelegationEdge[]): Map<string, number> {
  const nodeIds = new Set(nodes.map((node) => node.id));
  const incoming = new Map(nodes.map((node) => [node.id, 0]));
  const outgoing = new Map<string, string[]>();
  for (const edge of edges) {
    if (!nodeIds.has(edge.from) || !nodeIds.has(edge.to)) continue;
    incoming.set(edge.to, (incoming.get(edge.to) ?? 0) + 1);
    outgoing.set(edge.from, [...(outgoing.get(edge.from) ?? []), edge.to]);
  }
  const roots = nodes
    .filter((node) => node.kind === "root-agent" || (incoming.get(node.id) ?? 0) === 0)
    .sort((left, right) => left.id.localeCompare(right.id));
  const depths = new Map<string, number>();
  const queue = roots.map((node) => ({ id: node.id, depth: 0 }));
  while (queue.length) {
    const current = queue.shift();
    if (!current) break;
    const existing = depths.get(current.id);
    if (existing !== undefined && existing <= current.depth) continue;
    depths.set(current.id, current.depth);
    for (const child of (outgoing.get(current.id) ?? []).sort()) {
      queue.push({ id: child, depth: current.depth + 1 });
    }
  }
  for (const node of nodes) {
    if (!depths.has(node.id)) depths.set(node.id, 0);
  }
  return depths;
}

function statusColor(status: string, colors: DelegationGraphColors): string {
  if (status === "failed") return colors.failed;
  if (status === "waiting") return colors.waiting;
  if (status === "completed") return colors.completed;
  if (status === "started" || status === "running") return colors.active;
  return colors.unknown;
}

export function buildDelegationGraphOption({
  nodes,
  edges,
  selection,
  colors,
  relationLabel,
  statusLabel,
  restoreLabel,
}: {
  nodes: DelegationNode[];
  edges: DelegationEdge[];
  selection?: DelegationSelection;
  colors: DelegationGraphColors;
  relationLabel: (value: string) => string;
  statusLabel: (value: string) => string;
  restoreLabel: string;
}): echarts.EChartsCoreOption {
  const depths = nodeDepths(nodes, edges);
  const layers = new Map<number, DelegationNode[]>();
  for (const node of nodes) {
    const depth = depths.get(node.id) ?? 0;
    layers.set(depth, [...(layers.get(depth) ?? []), node]);
  }
  for (const layer of layers.values()) {
    layer.sort((left, right) =>
      (left.startedAt ?? "").localeCompare(right.startedAt ?? "") || left.id.localeCompare(right.id),
    );
  }

  const graphNodes = nodes.map((node) => {
    const depth = depths.get(node.id) ?? 0;
    const layer = layers.get(depth) ?? [node];
    const index = layer.findIndex((candidate) => candidate.id === node.id);
    const selected = selection?.type === "node" && selection.id === node.id;
    const inferred = node.evidenceLevel === "inferred";
    return {
      id: node.id,
      name: node.safeLabel,
      x: depth * 220,
      y: (index - (layer.length - 1) / 2) * 100,
      symbol: node.kind === "root-agent" ? "diamond" : node.kind === "subagent" ? "roundRect" : "circle",
      symbolSize: node.kind === "root-agent" ? 58 : 44,
      draggable: false,
      itemStyle: {
        color: colors.paper,
        borderColor: statusColor(node.status, colors),
        borderWidth: selected ? 5 : node.kind === "root-agent" ? 4 : 3,
        borderType: inferred ? "dashed" : "solid",
        shadowBlur: selected ? 16 : 0,
        shadowColor: statusColor(node.status, colors),
      },
      label: {
        show: true,
        position: "bottom",
        distance: 8,
        color: colors.text,
        fontSize: 11,
        fontWeight: node.kind === "root-agent" ? 700 : 500,
        width: 126,
        overflow: "truncate",
      },
      tooltip: {
        formatter: `${node.safeLabel}\n${statusLabel(node.status)} · ${node.evidenceLevel}`,
      },
    };
  });

  const graphEdges = edges.map((edge) => {
    const selected = selection?.type === "edge" && selection.id === edge.id;
    const inferred = edge.evidenceLevel === "inferred";
    const relationColor = colors.relation[edge.relationType] ?? colors.textSecondary;
    return {
      id: edge.id,
      source: edge.from,
      target: edge.to,
      symbol: ["none", "arrow"],
      symbolSize: [0, 9],
      lineStyle: {
        color: relationColor,
        width: selected ? 4 : 2,
        type: inferred || edge.relationType === "handoff" || edge.relationType === "resume" ? "dashed" : "solid",
        opacity: edge.confidence < 0.6 ? 0.58 : 0.9,
        curveness: edge.relationType === "join" || edge.relationType === "resume" ? -0.12 : 0.08,
      },
      label: {
        show: true,
        formatter: relationLabel(edge.relationType),
        color: relationColor,
        fontSize: 10,
        backgroundColor: colors.paper,
        padding: [2, 4],
        borderRadius: 4,
      },
      tooltip: {
        formatter: `${relationLabel(edge.relationType)}\n${statusLabel(edge.status)} · ${edge.evidenceLevel}`,
      },
    };
  });

  return {
    backgroundColor: "transparent",
    tooltip: { trigger: "item", renderMode: "richText" },
    toolbox: {
      show: true,
      right: 6,
      top: 6,
      feature: { restore: { show: true, title: restoreLabel } },
      iconStyle: { borderColor: colors.textSecondary },
      emphasis: { iconStyle: { borderColor: colors.text } },
    },
    series: [{
      type: "graph",
      layout: "none",
      data: graphNodes,
      links: graphEdges,
      roam: true,
      scaleLimit: { min: 0.35, max: 3 },
      edgeSymbol: ["none", "arrow"],
      emphasis: { focus: "adjacency" },
      select: { disabled: true },
      lineStyle: { color: colors.hairline },
    }],
  };
}
