import { describe, expect, it } from "vitest";
import { MAX_BUCKETS, MIN_STEP_PX, buildBuckets } from "./sparkBuckets";

const pt = (at: number, percent: number) => ({ at, percent });

describe("buildBuckets", () => {
  it("caps the bucket count at MAX_BUCKETS where the density floor does not bind", () => {
    // 1000 samples over 800px → density floor ceil(8/0.8) = 10 (100 buckets before the cap),
    // cap term ceil(1000/24) = 42 → 24 buckets. The cap term must win, or this proves nothing.
    const points = Array.from({ length: 1000 }, (_, i) => pt(i, 10 + (i % 50)));
    const buckets = buildBuckets(points, 800);
    expect(Math.ceil(MIN_STEP_PX / (800 / 999))).toBeLessThan(
      Math.ceil(points.length / MAX_BUCKETS),
    );
    expect(buckets.length).toBe(MAX_BUCKETS);
    // every sample is still covered exactly once
    expect(buckets[buckets.length - 1].end).toBe(points.length - 1);
  });

  it("still honours the density floor on a narrow chart", () => {
    const points = Array.from({ length: 1000 }, (_, i) => pt(i, 10 + (i % 50)));
    const buckets = buildBuckets(points, 40);
    expect(buckets.length).toBeLessThanOrEqual(MAX_BUCKETS);
    expect(buckets.length).toBeGreaterThan(1);
    expect(buckets[buckets.length - 1].end).toBe(points.length - 1);
  });

  it("keeps a median vertex so mid-bucket dips survive", () => {
    const points = [pt(0, 90), pt(1, 10), pt(2, 50), pt(3, 20), pt(4, 95)];
    const [bucket] = buildBuckets(points, 1); // 1px over 4 gaps → the whole series is one bucket
    expect(bucket.vertices).toEqual([1, 2, 4]); // min, median sample, max
  });

  it("locates the median by value, not by position", () => {
    // Sorted percents are 10, 20, 30, 90, 95 → the median sample is index 3 (30%),
    // while the positional middle is index 2 (20%): only a value median keeps the
    // vertex on the real middle of the bucket.
    const points = [pt(0, 10), pt(1, 90), pt(2, 20), pt(3, 30), pt(4, 95)];
    const [bucket] = buildBuckets(points, 1);
    expect(bucket.vertices).toEqual([0, 3, 4]);
    expect(bucket.percent).toBe(95); // the tooltip/peak still report the highest sample
  });

  it("degrades to one or two vertices when the bucket has fewer samples", () => {
    expect(buildBuckets([pt(0, 42)], 120)[0].vertices).toEqual([0]);
    const two = buildBuckets([pt(0, 10), pt(1, 20)], 1);
    expect(two[0].vertices).toEqual([0, 1]);
    expect(two[0].percent).toBe(20);
  });
});
