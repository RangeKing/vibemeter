import * as echarts from "echarts";
import { useEffect, useRef } from "react";
import type { CSSProperties } from "react";

interface EChartProps {
  option: echarts.EChartsCoreOption;
  ariaLabel: string;
  style?: CSSProperties;
  animated?: boolean;
}

export function EChart({ option, ariaLabel, style, animated = true }: EChartProps) {
  const ref = useRef<HTMLDivElement>(null);
  const chartRef = useRef<echarts.ECharts | null>(null);

  useEffect(() => {
    if (!ref.current) return;
    const chart = echarts.init(ref.current, undefined, { renderer: "canvas" });
    chartRef.current = chart;
    let resizeFrame = 0;
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(resizeFrame);
      resizeFrame = requestAnimationFrame(() => chart.resize());
    });
    observer.observe(ref.current);
    return () => {
      cancelAnimationFrame(resizeFrame);
      observer.disconnect();
      chartRef.current = null;
      chart.dispose();
    };
  }, []);

  useEffect(() => {
    const chart = chartRef.current;
    if (!chart) return;
    const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    chart.setOption({
      ...option,
      aria: { enabled: true, description: ariaLabel },
      animation: animated && !reduceMotion,
      animationDuration: animated ? 220 : 0,
      animationDurationUpdate: animated ? 160 : 0,
      animationEasing: "cubicOut",
      animationEasingUpdate: "cubicOut",
    }, { notMerge: true, lazyUpdate: true });
  }, [animated, option, ariaLabel]);

  return <div ref={ref} className="chart" style={style} role="img" aria-label={ariaLabel} />;
}
