import { VCTI_GUILD_ACCENT, type VctiGuild } from "../lib/vctiCatalog";
import type { VctiIdentityVisual as IdentityVisual } from "../types";

export function VctiIdentityVisual({
  visual,
  type,
  guild,
  label,
}: {
  visual: IdentityVisual;
  type?: string;
  guild?: string;
  label: string;
}) {
  const resolvedGuild = (guild || "start") as VctiGuild;
  return (
    <figure
      className={`vcti-identity-visual ${visual.available ? "available" : "collecting"}`}
      style={{ "--vcti-accent": VCTI_GUILD_ACCENT[resolvedGuild] } as React.CSSProperties}
      data-vcti-visual-version={visual.version}
      aria-label={label}
      role="img"
    >
      <svg viewBox="0 0 100 100" aria-hidden="true">
        <circle className="vcti-identity-orbit outer" cx="50" cy="50" r="44" />
        <circle className="vcti-identity-orbit inner" cx="50" cy="50" r="17" />
        <g className="vcti-identity-contours">
          {visual.paths.map((path, index) => (
            <path
              key={`${index}-${path.d}`}
              d={path.d}
              fill="none"
              stroke="currentColor"
              strokeWidth={path.strokeWidth}
              opacity={path.opacity}
            />
          ))}
        </g>
        <g className="vcti-identity-branches">
          {visual.branches.map((branch, index) => (
            <path
              key={`${index}-${branch.d}`}
              d={branch.d}
              fill="none"
              stroke="currentColor"
              strokeWidth={branch.strokeWidth}
              opacity={branch.opacity}
            />
          ))}
        </g>
        <g className="vcti-identity-details">
          {visual.details.map((detail, index) => (
            <circle
              key={index}
              cx={detail.cx}
              cy={detail.cy}
              r={detail.radius}
              fill="currentColor"
              opacity={detail.opacity}
            />
          ))}
        </g>
      </svg>
      {type ? <figcaption>{type}</figcaption> : null}
    </figure>
  );
}
