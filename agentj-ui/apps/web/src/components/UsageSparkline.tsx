import type { ReactElement } from "react";
import type { TrendPoint } from "@agentj/sdk";

type Props = {
  /** Points for one window kind, ascending by `at`; see `buildTrend`. */
  points: TrendPoint[];
  width?: number;
};

const HEIGHT = 16;
const PAD = 1;
const INNER = HEIGHT - PAD * 2;

/** Percent mapped into the padded band, so 0 % sits on the floor and 100 % on the ceiling. */
function pointY(percent: number): number {
  const clamped = Math.min(100, Math.max(0, percent));
  return HEIGHT - PAD - (clamped / 100) * INNER;
}

/**
 * Inline usage trend for one window kind. Renders nothing below two points — a single
 * sample is not a trend, and an empty chart reads as "no data" the wrong way.
 */
export default function UsageSparkline({ points, width = 120 }: Props): ReactElement | null {
  if (points.length < 2) return null;
  const step = width / (points.length - 1);
  const d = points
    .map((p, i) => `${i === 0 ? "M" : "L"}${(i * step).toFixed(1)},${pointY(p.percent).toFixed(1)}`)
    .join(" ");
  return (
    <svg className="aj-spark" width={width} height={HEIGHT} role="img" aria-label="用量趋势">
      <path d={d} fill="none" stroke="currentColor" strokeWidth="1.5" />
    </svg>
  );
}
