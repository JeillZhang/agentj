import type { TrendPoint } from "@agentj/sdk";

/** Below this per-sample spacing the chart buckets, so a dense series keeps its peak. */
export const MIN_STEP_PX = 8;

/**
 * Pack the samples into drawable slots. Up to one slot per sample while there is room
 * (`step >= MIN_STEP_PX`); beyond that each slot keeps its own min and max sample so a
 * spike survives even when 200 samples land in 120 px.
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
  const span = Math.max(2, Math.ceil(MIN_STEP_PX / step));
  const buckets: SparkBucket[] = [];
  for (let start = 0; start <= last; start += span) {
    const end = Math.min(last, start + span - 1);
    let low = start;
    let high = start;
    for (let i = start; i <= end; i += 1) {
      if (points[i].percent < points[low].percent) low = i;
      if (points[i].percent > points[high].percent) high = i;
    }
    const vertices = low === high ? [high] : [Math.min(low, high), Math.max(low, high)];
    buckets.push({ start, end, percent: points[high].percent, at: points[high].at, vertices });
  }
  return buckets;
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
  /** Original indices that stay vertices: the bucket's min and max, in index order. */
  vertices: number[];
};
