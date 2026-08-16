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
  /** agent tones: codex, claude, kimi, cursor, openclaw, hermes, zcode, DeepSeek Harness */
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

export const AGENT_CHART_FALLBACKS = [
  "#6750d8",
  "#d96845",
  "#1e6fd9",
  "#0c97a2",
  "#6f9827",
  "#b9439d",
  "#c98400",
  "#008c63",
] as const;

function readChartColors(): ChartColors {
  const styles = getComputedStyle(document.documentElement);
  const token = (name: string, fallback: string) => styles.getPropertyValue(name).trim() || fallback;
  const chart1 = token("--chart-1", AGENT_CHART_FALLBACKS[0]);
  const chart2 = token("--chart-2", AGENT_CHART_FALLBACKS[1]);
  const chart3 = token("--chart-3", AGENT_CHART_FALLBACKS[2]);
  const chart4 = token("--chart-4", "#8a6fc9");
  const warning = token("--warning", "#b97a26");
  const codex = token("--agent-codex", AGENT_CHART_FALLBACKS[0]);
  const claude = token("--agent-claude", AGENT_CHART_FALLBACKS[1]);
  const kimi = token("--agent-kimi", AGENT_CHART_FALLBACKS[2]);
  const cursor = token("--agent-cursor", AGENT_CHART_FALLBACKS[3]);
  const openclaw = token("--agent-openclaw", AGENT_CHART_FALLBACKS[4]);
  const hermes = token("--agent-hermes", AGENT_CHART_FALLBACKS[5]);
  const zcode = token("--agent-zcode", AGENT_CHART_FALLBACKS[6]);
  const deepseek = token("--agent-deepseek", AGENT_CHART_FALLBACKS[7]);
  const textTertiary = token("--text-tertiary", "#8b929e");
  return {
    series: [chart1, chart2, chart3, chart4, warning, textTertiary],
    agent: [codex, claude, kimi, cursor, openclaw, hermes, zcode, deepseek],
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
