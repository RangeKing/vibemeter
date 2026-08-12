import { ArrowRight, CircleHelp } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { VctiIdentityInput } from "../types";

const layers = [
  ["foundation", ["identity"]],
  ["structure", ["dimensions"]],
  ["rhythm", ["work-periods", "active-days", "session-density"]],
  ["branches", ["subagent-starts", "parallel-batches"]],
  ["detail", ["tool-categories", "explicit-skills"]],
  ["variation", ["errors", "retries", "rollbacks"]],
] as const;

export function VctiVisualLegend({
  inputs,
  onOpenEvidence,
}: {
  inputs: VctiIdentityInput[];
  onOpenEvidence: () => void;
}) {
  const { t } = useTranslation();
  const availability = new Map(inputs.map((input) => [input.id, input.available]));
  const missingCount = inputs.filter((input) => !input.available).length;
  return (
    <section className="vcti-visual-legend" aria-labelledby="vcti-visual-legend-title">
      <header>
        <div>
          <span className="eyebrow"><CircleHelp size={13} />{t("vcti.visualLegendEyebrow")}</span>
          <h2 id="vcti-visual-legend-title">{t("vcti.visualLegendTitle")}</h2>
          <p>{t("vcti.visualLegendBody")}</p>
        </div>
        <button className="button subtle" type="button" onClick={onOpenEvidence}>
          {t("vcti.openVisualEvidence")}<ArrowRight size={14} />
        </button>
      </header>
      {missingCount > 1 ? <p className="vcti-visual-missing-summary" role="status">{t("vcti.someBehaviorNotRecorded")}</p> : null}
      <ol>
        {layers.map(([layer, inputIds], index) => {
          const available = inputIds.every((id) => availability.get(id) === true);
          return (
            <li className={available ? "available" : "not-recorded"} key={layer}>
              <i>{String(index + 1).padStart(2, "0")}</i>
              <div>
                <strong>{t(`vcti.visualLayers.${layer}.title`)}</strong>
                <p>{t(`vcti.visualLayers.${layer}.body`)}</p>
                {!available ? <small>{t("vcti.notRecorded")}</small> : null}
              </div>
            </li>
          );
        })}
      </ol>
    </section>
  );
}
