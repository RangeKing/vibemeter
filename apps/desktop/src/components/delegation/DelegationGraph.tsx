import type * as echarts from "echarts";
import { Focus, Network } from "lucide-react";
import { useCallback, useMemo, useRef } from "react";
import { useTranslation } from "react-i18next";
import type { DelegationEdge, DelegationNode } from "../../types";
import { useChartColors } from "../../lib/chartTheme";
import { EChart } from "../EChart";
import { buildDelegationGraphOption, type DelegationSelection } from "./delegationGraphModel";

export function DelegationGraph({
  nodes,
  edges,
  selection,
  onSelection,
}: {
  nodes: DelegationNode[];
  edges: DelegationEdge[];
  selection?: DelegationSelection;
  onSelection: (selection: DelegationSelection) => void;
}) {
  const { t } = useTranslation();
  const chartColors = useChartColors();
  const chartRef = useRef<echarts.ECharts | null>(null);
  const colors = useMemo(() => ({
    text: chartColors.text,
    textSecondary: chartColors.textSecondary,
    hairline: chartColors.hairline,
    paper: chartColors.paper,
    active: chartColors.series[0],
    waiting: chartColors.series[4],
    failed: chartColors.series[1],
    completed: chartColors.positive,
    unknown: chartColors.textTertiary,
    relation: {
      delegate: chartColors.series[0],
      spawn: chartColors.series[2],
      handoff: chartColors.series[4],
      resume: chartColors.series[3],
      join: chartColors.positive,
    },
  }), [chartColors]);
  const option = useMemo(() => buildDelegationGraphOption({
    nodes,
    edges,
    selection,
    colors,
    relationLabel: (value) => t(`delegation.relation.${value}`),
    statusLabel: (value) => t(`delegation.status.${value}`),
    restoreLabel: t("delegation.graph.fit"),
  }), [colors, edges, nodes, selection, t]);
  const onChartClick = useCallback((event: echarts.ECElementEvent) => {
    const data = event.data as { id?: string } | undefined;
    if (!data?.id) return;
    if (event.dataType === "edge") onSelection({ type: "edge", id: data.id });
    else if (event.dataType === "node") onSelection({ type: "node", id: data.id });
  }, [onSelection]);
  const onReady = useCallback((chart: echarts.ECharts) => {
    chartRef.current = chart;
  }, []);

  if (!nodes.length) {
    return <div className="delegation-graph-empty"><Network size={20} /><span>{t("delegation.graph.noMatches")}</span></div>;
  }

  return (
    <section className="delegation-graph" aria-labelledby="delegation-graph-title">
      <header>
        <div>
          <h4 id="delegation-graph-title">{t("delegation.graph.title")}</h4>
          <p>{t("delegation.graph.body")}</p>
        </div>
        <button
          className="button subtle delegation-fit"
          onClick={() => chartRef.current?.dispatchAction({ type: "restore" })}
        >
          <Focus size={13} />
          {t("delegation.graph.fit")}
        </button>
      </header>
      <EChart
        option={option}
        ariaLabel={t("delegation.graph.aria", { nodes: nodes.length, edges: edges.length })}
        style={{ width: "100%", height: Math.min(640, Math.max(340, nodes.length * 46)) }}
        onClick={onChartClick}
        onReady={onReady}
      />
      <div className="delegation-graph-legend" aria-label={t("delegation.graph.legend")}>
        {(["running", "waiting", "failed", "completed"] as const).map((status) => (
          <span className={`status-${status}`} key={status}><i />{t(`delegation.status.${status}`)}</span>
        ))}
        <span className="evidence-inferred"><i />{t("delegation.evidence.inferred")}</span>
      </div>
      <details className="delegation-list-fallback" open={nodes.length > 30}>
        <summary>{t("delegation.graph.listFallback", { count: nodes.length + edges.length })}</summary>
        <div>
          <section aria-label={t("delegation.graph.nodes")}>
            {nodes.map((node) => (
              <button
                className={selection?.type === "node" && selection.id === node.id ? "selected" : ""}
                key={node.id}
                onClick={() => onSelection({ type: "node", id: node.id })}
              >
                <span>{node.safeLabel}</span>
                <small>{t(`delegation.status.${node.status}`)} · {t(`delegation.evidence.${node.evidenceLevel}`)}</small>
              </button>
            ))}
          </section>
          <section aria-label={t("delegation.graph.edges")}>
            {edges.map((edge) => (
              <button
                className={selection?.type === "edge" && selection.id === edge.id ? "selected" : ""}
                key={edge.id}
                onClick={() => onSelection({ type: "edge", id: edge.id })}
              >
                <span>{t(`delegation.relation.${edge.relationType}`)}</span>
                <small>{t(`delegation.status.${edge.status}`)} · {t(`delegation.evidence.${edge.evidenceLevel}`)}</small>
              </button>
            ))}
          </section>
        </div>
      </details>
    </section>
  );
}
