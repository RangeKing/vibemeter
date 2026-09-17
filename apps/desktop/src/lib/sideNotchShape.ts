/* Geometry ported from vinzdg/codenotch `SideNotchShape.canonicalPath`.
   See THIRD_PARTY_NOTICES.md. */

/* codenotch fixes the outline by three measurements in points: a 70 body depth,
   a 38.7 flare and a 29.6 corner — ratios of 1 : 0.553 : 0.423 against the
   depth. VibeMeter draws the same outline at 0.77 scale, which is what keeps
   the curve reading as the reference's while the strip stays slim enough to
   live on the edge of a working screen all day. Keep the three in step: the
   ratios, not the absolute numbers, are what the shape is. */

/** Across the notch, bezel to free side. */
export const SIDE_NOTCH_DEPTH = 54;
/** Radius of the inverse flare that welds each end back onto the bezel. */
export const SIDE_NOTCH_CURL_RADIUS = 30;
/** Radius of the two rounded corners on the free side of the body. */
export const SIDE_NOTCH_CORNER_RADIUS = 23;

export interface SideNotchGeometry {
  /** Across the shape. The bezel sits at `x = depth`. */
  depth?: number;
  /** Along the edge. */
  length: number;
  curlRadius?: number;
  cornerRadius?: number;
}

function round(value: number): number {
  return Math.round(value * 1000) / 1000;
}

/**
 * Canonical right-edge outline: a pill welded to the bezel, with inverse
 * rounded corners at each end that flare back out to the edge so it reads as
 * part of the bezel rather than a floating panel. Mirror it on x for the left
 * edge instead of writing a second variant.
 *
 * The clamping order matters. Clamping the corner by `depth - curl` — the
 * obvious reading — collapses it to zero as soon as the flare is as wide as the
 * body, which is exactly what happens when the notch folds to its pill. The
 * corner is claimed first, out of half the depth, and the flare takes the rest.
 */
export function sideNotchPath({
  depth = SIDE_NOTCH_DEPTH,
  length,
  curlRadius = SIDE_NOTCH_CURL_RADIUS,
  cornerRadius = SIDE_NOTCH_CORNER_RADIUS,
}: SideNotchGeometry): string {
  const width = Math.max(0, depth);
  const height = Math.max(0, length);
  const wanted = Math.max(0, Math.min(cornerRadius, width / 2));
  const curl = Math.max(0, Math.min(curlRadius, height / 2, width - wanted));
  const corner = Math.max(0, Math.min(wanted, (height - 2 * curl) / 2));
  const bodyTop = round(curl);
  const bodyBottom = round(height - curl);
  const w = round(width);
  const h = round(height);
  const c = round(corner);
  const k = round(curl);

  const segments = [`M ${w} 0`];
  // Flare inward and down onto the top edge. Sweep 1: the arc curves away from
  // the body, so the shape grows out of the bezel. Sweep 0 here rounds the end
  // off into a floating capsule instead. Absent when flush.
  if (curl > 0) segments.push(`A ${k} ${k} 0 0 1 ${round(width - curl)} ${bodyTop}`);
  segments.push(`L ${c} ${bodyTop}`);
  if (corner > 0) segments.push(`A ${c} ${c} 0 0 0 0 ${round(curl + corner)}`);
  segments.push(`L 0 ${round(height - curl - corner)}`);
  if (corner > 0) segments.push(`A ${c} ${c} 0 0 0 ${c} ${bodyBottom}`);
  segments.push(`L ${round(width - curl)} ${bodyBottom}`);
  // Flare back out to the bezel.
  if (curl > 0) segments.push(`A ${k} ${k} 0 0 1 ${w} ${h}`);
  segments.push("Z");
  return segments.join(" ");
}

/** Mirrors the canonical path onto the left edge. */
export function sideNotchTransform(side: "left" | "right", depth: number): string | undefined {
  return side === "left" ? `translate(${round(depth)} 0) scale(-1 1)` : undefined;
}
