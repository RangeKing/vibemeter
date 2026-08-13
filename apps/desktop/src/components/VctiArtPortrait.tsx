import { useEffect, useState, type CSSProperties } from "react";
import { VCTI_GUILD_ACCENT, type VctiGuild } from "../lib/vctiCatalog";
import type { VctiIdentityVisual } from "../types";
import { VctiAvatar } from "./VctiAvatar";

type VctiArtProps = {
  visual: VctiIdentityVisual;
  type?: string;
  guild?: string;
  label: string;
};

function accentStyle(guild?: string) {
  const resolvedGuild = (guild || "start") as VctiGuild;
  return { "--vcti-accent": VCTI_GUILD_ACCENT[resolvedGuild] } as CSSProperties;
}

export function VctiArtField({ visual, type, guild }: Pick<VctiArtProps, "visual" | "type" | "guild">) {
  const reduceMotion = typeof window !== "undefined" && typeof window.matchMedia === "function"
    ? window.matchMedia("(prefers-reduced-motion: reduce)").matches
    : false;
  const [generating, setGenerating] = useState(!reduceMotion);

  useEffect(() => {
    if (reduceMotion || !visual.available || !type) {
      setGenerating(false);
      return;
    }
    setGenerating(true);
    const timeout = window.setTimeout(() => setGenerating(false), 800);
    return () => window.clearTimeout(timeout);
  }, [reduceMotion, type, visual.algorithmVersion, visual.range, visual.version, visual.available]);

  if (!visual.available || !type) return null;

  return (
    <svg
      className="vcti-art-field"
      viewBox="0 0 160 100"
      preserveAspectRatio="xMidYMid slice"
      style={accentStyle(guild)}
      data-vcti-visual-version={visual.version}
      data-vcti-range={visual.range}
      data-generating={generating ? "true" : "false"}
      aria-hidden="true"
    >
      <defs>
        <linearGradient id="vcti-terrain-fade" x1="0" x2="1">
          <stop offset="0" stopColor="white" stopOpacity=".06" />
          <stop offset=".34" stopColor="white" stopOpacity=".12" />
          <stop offset=".52" stopColor="white" stopOpacity=".5" />
          <stop offset=".68" stopColor="white" stopOpacity=".92" />
          <stop offset="1" stopColor="white" />
        </linearGradient>
        <mask id="vcti-terrain-mask"><rect width="160" height="100" fill="url(#vcti-terrain-fade)" /></mask>
      </defs>
      <g className="vcti-terrain" mask="url(#vcti-terrain-mask)">
      <g className="vcti-art-contours" transform="translate(69 0)" data-visual-channel="topology">
        {visual.contours.map((path, index) => (
          <path key={`${index}-${path.d}`} d={path.d} fill="none" stroke="currentColor" strokeWidth={path.strokeWidth} opacity={Math.max(.38, path.opacity * .9)} vectorEffect="non-scaling-stroke" />
        ))}
      </g>
      <g className="vcti-art-rhythm" transform="translate(25 0) scale(1.25 1)" data-visual-channel="rhythm">
        {visual.rhythm.paths.map((path, index) => (
          <path key={`${index}-${path.d}`} d={path.d} fill="none" stroke="currentColor" strokeWidth={path.strokeWidth * .72} opacity={Math.max(.1, path.opacity * .28)} vectorEffect="non-scaling-stroke" />
        ))}
      </g>
      <g className="vcti-art-branches" transform="translate(25 0) scale(1.25 1)" data-visual-channel="branches">
        {visual.collaboration.paths.map((path, index) => (
          <path key={`${index}-${path.d}`} d={path.d} fill="none" stroke="currentColor" strokeWidth={path.strokeWidth * .8} opacity={Math.max(.2, path.opacity * .38)} vectorEffect="non-scaling-stroke" />
        ))}
      </g>
      <g className="vcti-art-detail" transform="translate(25 0) scale(1.25 1)" data-visual-channel="detail">
        <g className="vcti-art-tools">
          {visual.detail.toolMarks.map((mark, index) => <path data-mark="tool" key={index} d={`M${mark.cx - .45},${mark.cy - .65} L${mark.cx + .45},${mark.cy + .65}`} fill="none" stroke="currentColor" strokeWidth=".55" opacity={Math.max(.24, mark.opacity * .45)} vectorEffect="non-scaling-stroke" />)}
        </g>
        <g className="vcti-art-skills">
          {visual.detail.skillMarks.map((mark, index) => <path data-mark="skill" key={index} d={`M${mark.cx - .6},${mark.cy - .72} L${mark.cx + .6},${mark.cy + .72} M${mark.cx - .6},${mark.cy + .72} L${mark.cx + .6},${mark.cy - .72}`} fill="none" stroke="currentColor" strokeWidth=".5" opacity={Math.max(.28, mark.opacity * .42)} vectorEffect="non-scaling-stroke" />)}
        </g>
      </g>
      <g className="vcti-art-process" transform="translate(25 0) scale(1.25 1)" data-visual-channel="process">
        {visual.process.paths.map((path, index) => <path key={`${index}-${path.d}`} d={path.d} fill="none" stroke="currentColor" strokeWidth={path.strokeWidth * .72} opacity={Math.max(.14, path.opacity * .3)} vectorEffect="non-scaling-stroke" />)}
      </g>
      </g>
    </svg>
  );
}

export function VctiArtPortrait({ visual, type, guild, label }: VctiArtProps) {
  if (!visual.available || !type) return <VctiAvatar type={type} guild={guild} label={label} />;
  return (
    <figure
      className="vcti-art-portrait"
      style={accentStyle(guild)}
      data-vcti-visual-version={visual.version}
      data-vcti-range={visual.range}
      aria-label={`${label} · VCTI`}
    >
      <VctiAvatar type={type} guild={guild} label={label} />
    </figure>
  );
}
