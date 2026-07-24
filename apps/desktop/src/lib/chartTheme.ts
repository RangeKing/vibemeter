import { useEffect, useState } from "react";

/**
 * Chart colors must follow the active theme. CSS custom properties already
 * encode light/dark variants, so charts read the resolved values instead of
 * duplicating hex constants. The hook re-reads them when the `data-theme`
 * attribute or the OS color scheme changes.
 */
export type ChartColors = {
  /** ordered categorical tones for generic series (treemap, bars) */
  series: string[];
  /** agent tones: codex, claude, kimi, cursor, openclaw, hermes */
  agent: string[];
  /** model ring tones */
  model: string[];
  text: string;
  textSecondary: string;
  textTertiary: string;
  hairline: string;
  paper: string;
  positive: string;
};

function readChartColors(): ChartColors {
  const styles = getComputedStyle(document.documentElement);
  const token = (name: string, fallback: string) => styles.getPropertyValue(name).trim() || fallback;
  const chart1 = token("--chart-1", "#3b5bdb");
  const chart2 = token("--chart-2", "#b96a3c");
  const chart3 = token("--chart-3", "#4c9a7a");
  const chart4 = token("--chart-4", "#8a6fc9");
  const warning = token("--warning", "#b97a26");
  const cursor = token("--agent-cursor", "#3e8ea4");
  const openclaw = token("--agent-openclaw", "#b6792e");
  const hermes = token("--agent-hermes", "#a85c91");
  const textTertiary = token("--text-tertiary", "#8b929e");
  return {
    series: [chart1, chart2, chart3, chart4, warning, textTertiary],
    agent: [chart1, chart2, chart3, cursor, openclaw, hermes],
    model: [chart3, chart4, warning, textTertiary, chart1, chart2],
    text: token("--text", "#1d2129"),
    textSecondary: token("--text-secondary", "#59616d"),
    textTertiary,
    hairline: token("--hairline", "rgba(38, 48, 66, 0.1)"),
    paper: token("--paper-solid", "#fcfbfa"),
    positive: token("--positive", "#3e9068"),
  };
}

export function useChartColors(): ChartColors {
  const [colors, setColors] = useState(readChartColors);
  useEffect(() => {
    const update = () => setColors(readChartColors());
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    media.addEventListener("change", update);
    const observer = new MutationObserver(update);
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    return () => {
      media.removeEventListener("change", update);
      observer.disconnect();
    };
  }, []);
  return colors;
}

/** Shared tooltip chrome so charts match the app's paper surfaces. */
export function chartTooltip(colors: ChartColors) {
  return {
    renderMode: "html" as const,
    appendTo: "body",
    confine: false,
    backgroundColor: colors.paper,
    borderColor: colors.hairline,
    borderWidth: 1,
    padding: [8, 10] as [number, number],
    textStyle: { color: colors.text, fontSize: 11 },
    extraCssText: "z-index: 1000; box-shadow: 0 12px 34px rgba(0, 0, 0, 0.20); border-radius: 10px; pointer-events: none;",
  };
}
