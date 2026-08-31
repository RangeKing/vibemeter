import { RotateCcw } from "lucide-react";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import type { DelegationEdge, DelegationNode } from "../../types";
import type { DelegationFilterValue } from "./delegationGraphModel";

export function DelegationFilters({
  nodes,
  edges,
  value,
  onChange,
}: {
  nodes: DelegationNode[];
  edges: DelegationEdge[];
  value: DelegationFilterValue;
  onChange: (value: DelegationFilterValue) => void;
}) {
  const { t } = useTranslation();
  const agents = useMemo(() => [...new Set(nodes.map((node) => node.agent))].sort(), [nodes]);
  const relationTypes = useMemo(() => [...new Set(edges.map((edge) => edge.relationType))].sort(), [edges]);
  const statuses = useMemo(
    () => [...new Set([...nodes.map((node) => node.status), ...edges.map((edge) => edge.status)])].sort(),
    [edges, nodes],
  );
  const active = value.agent !== "all" || value.relationType !== "all" || value.status !== "all";

  return (
    <div className="delegation-filters" aria-label={t("delegation.filters.title")}>
      <label>
        <span>{t("delegation.filters.agent")}</span>
        <select
          value={value.agent}
          onChange={(event) => onChange({ ...value, agent: event.target.value })}
        >
          <option value="all">{t("delegation.filters.allAgents")}</option>
          {agents.map((agent) => <option value={agent} key={agent}>{agent}</option>)}
        </select>
      </label>
      <label>
        <span>{t("delegation.filters.relation")}</span>
        <select
          value={value.relationType}
          onChange={(event) => onChange({ ...value, relationType: event.target.value })}
        >
          <option value="all">{t("delegation.filters.allRelations")}</option>
          {relationTypes.map((relation) => (
            <option value={relation} key={relation}>{t(`delegation.relation.${relation}`)}</option>
          ))}
        </select>
      </label>
      <label>
        <span>{t("delegation.filters.status")}</span>
        <select
          value={value.status}
          onChange={(event) => onChange({ ...value, status: event.target.value })}
        >
          <option value="all">{t("delegation.filters.allStatuses")}</option>
          {statuses.map((status) => (
            <option value={status} key={status}>{t(`delegation.status.${status}`)}</option>
          ))}
        </select>
      </label>
      <button
        className="delegation-filter-reset"
        disabled={!active}
        onClick={() => onChange({ agent: "all", relationType: "all", status: "all" })}
      >
        <RotateCcw size={12} />
        {t("delegation.filters.reset")}
      </button>
    </div>
  );
}
