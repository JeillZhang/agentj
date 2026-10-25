import { useState } from "react";
import type { KeyboardEvent, ReactElement } from "react";
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

function pct(percent: number): number {
  return Math.round(percent);
}

/** Screen-reader summary; also the live region's idle text and the paragraph fallback. */
function summarise(points: TrendPoint[]): string {
  const values = points.map((p) => pct(p.percent));
  const latest = values[values.length - 1];
  return `用量趋势：最新 ${latest}%，最低 ${Math.min(...values)}%，最高 ${Math.max(...values)}%`;
}

function describePoint(point: TrendPoint): string {
  return `${pct(point.percent)}% · ${new Date(point.at * 1000).toLocaleString()}`;
}

/**
 * Inline usage trend for one window kind. Renders nothing below two points — a single
 * sample is not a trend, and an empty chart reads as "no data" the wrong way.
 *
 * Hovering a vertical band inspects that sample; the same inspection is reachable from the
 * keyboard (focus the chart, step with ←/→, Esc to dismiss) and announced through a live
 * region, so the readout is not pointer-only.
 */
export default function UsageSparkline({ points, width = 120 }: Props): ReactElement | null {
  const [active, setActive] = useState<number | null>(null);
  if (points.length < 2) return null;
  const last = points.length - 1;
  const current = active !== null && active >= 0 && active <= last ? active : null;
  const step = width / last;
  const summary = summarise(points);
  const tip = current === null ? null : describePoint(points[current]);

  const stepBy = (delta: number) => {
    const from = current ?? last;
    setActive(Math.min(last, Math.max(0, from + delta)));
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "ArrowLeft") {
      event.preventDefault();
      stepBy(-1);
    } else if (event.key === "ArrowRight") {
      event.preventDefault();
      stepBy(1);
    } else if (event.key === "Home") {
      event.preventDefault();
      setActive(0);
    } else if (event.key === "End") {
      event.preventDefault();
      setActive(last);
    } else if (event.key === "Escape") {
      setActive(null);
    }
  };

  const d = points
    .map((p, i) => `${i === 0 ? "M" : "L"}${(i * step).toFixed(1)},${pointY(p.percent).toFixed(1)}`)
    .join(" ");
  return (
    <div
      className="aj-spark-wrap"
      data-testid="spark-keyboard"
      role="group"
      tabIndex={0}
      aria-label={`${summary}（聚焦后可用左右方向键逐点查看）`}
      onKeyDown={onKeyDown}
      onFocus={() => setActive((a) => a ?? last)}
      onBlur={() => setActive(null)}
    >
      <svg className="aj-spark" width={width} height={HEIGHT} role="img" aria-label={summary}>
        <path d={d} fill="none" stroke="currentColor" strokeWidth="1.5" />
        {points.map((p, i) => (
          <rect
            key={`${p.at}-${i}`}
            className="aj-spark-hit"
            data-testid="spark-hit"
            x={Math.max(0, i * step - step / 2)}
            y={0}
            width={step}
            height={HEIGHT}
            fill="transparent"
            onMouseEnter={() => setActive(i)}
            onMouseLeave={() => setActive((a) => (a === i ? null : a))}
          >
            <title>{describePoint(p)}</title>
          </rect>
        ))}
      </svg>
      {tip !== null && current !== null && (
        <div
          className="aj-spark-tip"
          role="tooltip"
          style={{ left: `${Math.round(((current * step) / width) * 100)}%` }}
        >
          {tip}
        </div>
      )}
      <span className="aj-spark-live" data-testid="spark-live" aria-live="polite">
        {tip ?? summary}
      </span>
    </div>
  );
}
