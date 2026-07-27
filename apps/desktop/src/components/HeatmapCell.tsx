import type { CSSProperties, KeyboardEvent } from "react";

type HeatmapCellProps = {
  tooltip: string;
  ariaLabel: string;
  style?: CSSProperties;
  index: number;
  total: number;
  active: boolean;
  onActivate: (index: number) => void;
  className?: string;
};

export function HeatmapCell({
  tooltip,
  ariaLabel,
  style,
  index,
  total,
  active,
  onActivate,
  className = "heatmap-cell",
}: HeatmapCellProps) {
  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key === "ArrowRight" || event.key === "ArrowDown") {
      event.preventDefault();
      onActivate(Math.min(total - 1, index + 1));
    } else if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
      event.preventDefault();
      onActivate(Math.max(0, index - 1));
    } else if (event.key === "Home") {
      event.preventDefault();
      onActivate(0);
    } else if (event.key === "End") {
      event.preventDefault();
      onActivate(total - 1);
    }
  };

  return (
    <button
      type="button"
      className={className}
      tabIndex={active ? 0 : -1}
      data-heatmap-index={index}
      data-tooltip={tooltip}
      aria-label={ariaLabel}
      style={style}
      onFocus={() => onActivate(index)}
      onKeyDown={onKeyDown}
    />
  );
}

export function focusHeatmapIndex(root: HTMLElement | null, index: number) {
  const cell = root?.querySelector<HTMLElement>(`[data-heatmap-index="${index}"]`);
  cell?.focus();
}
