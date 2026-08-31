import { GitMerge, GitPullRequestArrow, Timer } from "lucide-react";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import type { DelegationEdge, DelegationNode, Locale } from "../../types";
import type { DelegationSelection } from "./delegationGraphModel";

function timestamp(value: string | undefined): number | undefined {
  if (!value) return undefined;
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : undefined;
}

function timeLabel(value: string | undefined, locale: Locale): string {
  const parsed = timestamp(value);
  if (parsed === undefined) return "—";
  return new Intl.DateTimeFormat(locale, {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  }).format(parsed);
}

export function DelegationTimeline({
  nodes,
  edges,
  locale,
  selection,
  onSelection,
}: {
  nodes: DelegationNode[];
  edges: DelegationEdge[];
  locale: Locale;
  selection?: DelegationSelection;
  onSelection: (selection: DelegationSelection) => void;
}) {
  const { t } = useTranslation();
  const scale = useMemo(() => {
    const values = nodes
      .flatMap((node) => [timestamp(node.startedAt), timestamp(node.endedAt)])
      .filter((value): value is number => value !== undefined);
    const minimum = values.length ? Math.min(...values) : 0;
    const maximum = values.length ? Math.max(...values) : minimum;
    return { minimum, duration: Math.max(1, maximum - minimum) };
  }, [nodes]);
  const markerEdges = edges.filter((edge) => edge.relationType === "handoff" || edge.relationType === "join");
  const nodeById = new Map(nodes.map((node) => [node.id, node]));

  if (!nodes.length) return null;

  return (
    <section className="delegation-timeline" aria-labelledby="delegation-timeline-title">
      <header>
        <div>
          <h4 id="delegation-timeline-title">{t("delegation.timeline.title")}</h4>
          <p>{t("delegation.timeline.body")}</p>
        </div>
        <span><Timer size={13} />{t("delegation.timeline.branchCount", { count: nodes.length })}</span>
      </header>
      <div className="delegation-timeline-grid">
        {nodes.map((node) => {
          const started = timestamp(node.startedAt);
          const ended = timestamp(node.endedAt);
          const left = started === undefined ? 0 : ((started - scale.minimum) / scale.duration) * 100;
          const right = ended === undefined ? 100 : ((ended - scale.minimum) / scale.duration) * 100;
          const width = Math.max(1.5, right - left);
          const selected = selection?.type === "node" && selection.id === node.id;
          return (
            <button
              className={`delegation-life-row status-${node.status} ${selected ? "selected" : ""}`}
              key={node.id}
              onClick={() => onSelection({ type: "node", id: node.id })}
              aria-label={`${node.safeLabel} · ${t(`delegation.status.${node.status}`)}`}
            >
              <span className="delegation-life-label"><strong>{node.safeLabel}</strong><small>{timeLabel(node.startedAt, locale)}–{timeLabel(node.endedAt, locale)}</small></span>
              <span className="delegation-life-track" aria-hidden="true">
                <i style={{ left: `${left}%`, width: `${width}%` }} />
              </span>
            </button>
          );
        })}
      </div>
      {markerEdges.length ? (
        <div className="delegation-transition-list" aria-label={t("delegation.timeline.transitions")}>
          {markerEdges.map((edge) => {
            const target = nodeById.get(edge.to);
            const selected = selection?.type === "edge" && selection.id === edge.id;
            const Icon = edge.relationType === "handoff" ? GitPullRequestArrow : GitMerge;
            return (
              <button
                className={selected ? "selected" : ""}
                key={edge.id}
                onClick={() => onSelection({ type: "edge", id: edge.id })}
              >
                <Icon size={13} />
                <span>{t(`delegation.relation.${edge.relationType}`)}</span>
                <small>{target?.safeLabel ?? t("delegation.unknownAgent")} · {t(`delegation.status.${edge.status}`)}</small>
              </button>
            );
          })}
        </div>
      ) : null}
    </section>
  );
}
