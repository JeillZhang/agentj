// agentj-ui/packages/sdk/src/__tests__/usageTrend.test.ts
// Shared by the Web subscription panel and the TUI subscription overlay — the trend
// semantics live here so both frontends read the same samples the same way.
import { describe, expect, test } from "vitest";
import { buildTrend, sparkline } from "../usageTrend.ts";

const s = (at: number, kind: string, percent: number | null) => ({
  at,
  windows: [{ kind, percent }],
});

describe("buildTrend", () => {
  test("returns nothing without matching samples", () => {
    expect(buildTrend([], "rolling", 100)).toEqual([]);
    expect(buildTrend([s(1, "weekly", 5)], "rolling", 100)).toEqual([]);
  });

  test("keeps only the requested kind, ascending, numeric percents", () => {
    const out = buildTrend(
      [s(30, "rolling", 30), s(10, "rolling", 10), s(20, "weekly", 20), s(40, "rolling", null)],
      "rolling",
      100,
    );
    expect(out).toEqual([
      { at: 10, percent: 10 },
      { at: 30, percent: 30 },
    ]);
  });

  test("drops future samples and clamps the percent", () => {
    expect(buildTrend([s(200, "rolling", 55), s(50, "rolling", 140)], "rolling", 100)).toEqual([
      { at: 50, percent: 100 },
    ]);
  });

  test("downsamples to at most `max` points and keeps both ends", () => {
    const many = Array.from({ length: 100 }, (_, i) => s(i, "rolling", i % 100));
    const out = buildTrend(many, "rolling", 1_000, 10);
    expect(out).toHaveLength(10);
    expect(out[0].at).toBe(0);
    expect(out[9].at).toBe(99);
  });
});

describe("sparkline", () => {
  test("maps values onto the eight block glyphs", () => {
    expect(sparkline([0, 100], 8)).toBe("▁█");
    expect(sparkline([], 8)).toBe("");
    expect(sparkline([50], 4)).toBe("▄");
  });

  test("never exceeds the requested width", () => {
    expect(sparkline([10, 20, 30, 40, 50], 3)).toHaveLength(3);
  });
});
