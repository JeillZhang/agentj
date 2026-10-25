// agentj-ui/packages/sdk/src/usageTrend.ts
/** One point of a usage trend, already filtered to a single window kind. */
export type TrendPoint = { at: number; percent: number };

/** The subset of a `provider/usageHistory` sample the trend needs. */
export type UsageSampleLike = {
  at: number;
  windows: Array<{ kind: string; percent?: number | null }>;
};

const GLYPHS = "▁▂▃▄▅▆▇█";
const TOP_GLYPH = GLYPHS.length - 1;
const DEFAULT_MAX_POINTS = 2000;

function clampPercent(value: number): number {
  return Math.min(100, Math.max(0, Math.round(value)));
}

/**
 * Percent of the requested window in one sample, or `null` when the sample carries no
 * usable percent for it. An empty `kind` filters nothing — the `Option::None` analogue of
 * the kernel query — so the first window with a numeric percent stands in.
 */
function samplePercent(sample: UsageSampleLike, kind: string): number | null {
  const windows = Array.isArray(sample.windows) ? sample.windows : [];
  for (const window of windows) {
    if (kind !== "" && window.kind !== kind) continue;
    const raw = window.percent;
    if (typeof raw !== "number" || !Number.isFinite(raw)) continue;
    return clampPercent(raw);
  }
  return null;
}

/** Equal-spaced downsample that always keeps the first and the last entry. */
function equalSpaced<T>(items: T[], max: number): T[] {
  const limit = Number.isFinite(max) ? Math.floor(max) : items.length;
  if (limit >= items.length) return items;
  if (limit <= 0) return [];
  if (limit === 1) return items.slice(0, 1);
  const last = items.length - 1;
  const out: T[] = [];
  for (let i = 0; i < limit; i += 1) out.push(items[Math.round((i * last) / (limit - 1))]);
  return out;
}

/**
 * Ascending trend for one window kind: future samples, foreign kinds and samples without a
 * numeric percent are dropped rather than drawn as 0, then the page is downsampled to `max`.
 */
export function buildTrend(
  samples: UsageSampleLike[],
  kind: string,
  now: number,
  max = DEFAULT_MAX_POINTS,
): TrendPoint[] {
  const points: TrendPoint[] = [];
  for (const sample of samples) {
    if (!Number.isFinite(sample.at) || sample.at > now) continue;
    const percent = samplePercent(sample, kind);
    if (percent === null) continue;
    points.push({ at: sample.at, percent });
  }
  points.sort((a, b) => a.at - b.at);
  return equalSpaced(points, max);
}

/** One glyph per value: seven bands across eight glyphs, so 0 % is `▁` and 100 % is `█`. */
function glyphFor(value: number): string {
  if (!Number.isFinite(value)) return GLYPHS[0];
  const band = Math.floor((clampPercent(value) / 100) * TOP_GLYPH);
  return GLYPHS[Math.min(TOP_GLYPH, band)];
}

/** Block-character trend of `values`; `width` caps the glyph count and never pads. */
export function sparkline(values: number[], width: number): string {
  let out = "";
  for (const value of equalSpaced(values, width)) out += glyphFor(value);
  return out;
}
