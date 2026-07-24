const SAVITZKY_GOLAY_7 = [-2 / 21, 3 / 21, 6 / 21, 7 / 21, 6 / 21, 3 / 21, -2 / 21] as const;
const SAVITZKY_GOLAY_5 = [-3 / 35, 12 / 35, 17 / 35, 12 / 35, -3 / 35] as const;

function mirrorIndex(index: number, length: number): number {
  if (index < 0) return -index;
  if (index >= length) return 2 * length - index - 2;
  return index;
}

/**
 * Applies a second-order Savitzky–Golay filter to evenly spaced usage samples.
 * Seven points are preferred; five points are used for shorter series.
 * Mirrored boundaries match the filter's interior behavior without padding zeros.
 */
export function savitzkyGolaySmooth(values: readonly number[]): number[] {
  const kernel = values.length >= 7
    ? SAVITZKY_GOLAY_7
    : values.length >= 5
      ? SAVITZKY_GOLAY_5
      : undefined;
  if (!kernel) return values.map((value) => Math.max(0, Number.isFinite(value) ? value : 0));

  const radius = Math.floor(kernel.length / 2);
  return values.map((_, index) => {
    const filtered = kernel.reduce((sum, coefficient, offset) => {
      const sourceIndex = mirrorIndex(index + offset - radius, values.length);
      const value = values[sourceIndex];
      return sum + coefficient * (Number.isFinite(value) ? value : 0);
    }, 0);
    return Math.max(0, filtered);
  });
}
