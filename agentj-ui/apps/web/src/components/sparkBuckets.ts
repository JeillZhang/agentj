import type { TrendPoint } from "@agentj/sdk";

/** Below this per-sample spacing the chart buckets, so a dense series keeps its peak. */
export const MIN_STEP_PX = 8;

/**
 * Hard cap on buckets: the density floor alone would ask for hundreds of slots on a
 * narrow chart, which draws slower and makes the tooltip steps microscopic.
 */
export const MAX_BUCKETS = 24;

/**
 * Pack the samples into drawable slots. Up to one slot per sample while there is room
 * (`step >= MIN_STEP_PX`); beyond that each slot keeps its own min, median and max sample
 * so a spike or a mid-bucket dip survives even when 200 samples land in 120 px. The slot
 * count is capped at [`MAX_BUCKETS`] so a narrow chart stays drawable.
 */
export function buildBuckets(points: TrendPoint[], width: number): SparkBucket[] {
  const last = points.length - 1;
  const step = last > 0 ? width / last : 0;
  if (!(step > 0) || step >= MIN_STEP_PX) {
    return points.map((point, i) => ({
      start: i,
      end: i,
      percent: point.percent,
      at: point.at,
      vertices: [i],
    }));
  }
  const span = Math.max(
    2,
    Math.ceil(MIN_STEP_PX / step),
    Math.ceil(points.length / MAX_BUCKETS),
  );
  const buckets: SparkBucket[] = [];
  for (let start = 0; start <= last; start += span) {
    const end = Math.min(last, start + span - 1);
    let low = start;
    let high = start;
    for (let i = start; i <= end; i += 1) {
      if (points[i].percent < points[low].percent) low = i;
      if (points[i].percent > points[high].percent) high = i;
    }
    const mid = medianIndex(points, start, end);
    const vertices = [...new Set([low, mid, high])].sort((a, b) => a - b);
    buckets.push({ start, end, percent: points[high].percent, at: points[high].at, vertices });
  }
  return buckets;
}

/**
 * Index of the bucket's *median* sample: the middle of its percents by value, not by
 * position, so a dip or a climb inside the bucket keeps a vertex on the real middle.
 */
function medianIndex(points: TrendPoint[], start: number, end: number): number {
  const order: number[] = [];
  for (let i = start; i <= end; i += 1) order.push(i);
  order.sort((a, b) => points[a].percent - points[b].percent || a - b);
  return order[Math.floor((order.length - 1) / 2)];
}

/** One drawable/interactive slot: a single sample, or a bucket of neighbouring samples. */
export type SparkBucket = {
  /** First original index covered (inclusive). */
  start: number;
  /** Last original index covered (inclusive). */
  end: number;
  /** Highest percent in the bucket — what the shape and the tooltip report. */
  percent: number;
  /** Timestamp of that highest sample. */
  at: number;
  /** Original indices that stay vertices: the bucket's min, median and max, in index order. */
  vertices: number[];
};
