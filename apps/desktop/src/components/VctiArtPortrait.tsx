import type { CSSProperties } from "react";
import { VCTI_GUILD_ACCENT, type VctiGuild } from "../lib/vctiCatalog";
import type { VctiIdentityVisual } from "../types";
import { VctiAvatar } from "./VctiAvatar";

export function VctiArtPortrait({ visual, type, guild, label }: {
  visual: VctiIdentityVisual;
  type?: string;
  guild?: string;
  label: string;
}) {
  if (!visual.available || !type) return <VctiAvatar type={type} guild={guild} label={label} />;
  const resolvedGuild = (guild || "start") as VctiGuild;
  return (
    <div
      className="vcti-art-portrait"
      style={{ "--vcti-accent": VCTI_GUILD_ACCENT[resolvedGuild] } as CSSProperties}
      data-vcti-visual-version={visual.version}
      data-vcti-range={visual.range}
    >
      <svg className="vcti-art-field" viewBox="0 0 100 100" aria-hidden="true">
        <g className="vcti-art-contours">
          {visual.contours.map((path, index) => (
            <path key={`${index}-${path.d}`} d={path.d} fill="none" stroke="currentColor" strokeWidth={path.strokeWidth} opacity={path.opacity} />
          ))}
        </g>
        <g className="vcti-art-rhythm">
          {visual.rhythm.paths.map((path, index) => (
            <path key={`${index}-${path.d}`} d={path.d} fill="none" stroke="currentColor" strokeWidth={path.strokeWidth} opacity={path.opacity} />
          ))}
        </g>
        <g className="vcti-art-branches">
          {visual.collaboration.paths.map((path, index) => (
            <path key={`${index}-${path.d}`} d={path.d} fill="none" stroke="currentColor" strokeWidth={path.strokeWidth} opacity={path.opacity} />
          ))}
        </g>
      </svg>
      <VctiAvatar type={type} guild={guild} label={label} />
    </div>
  );
}
