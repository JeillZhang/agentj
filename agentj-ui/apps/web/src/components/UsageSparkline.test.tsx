import { describe, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import UsageSparkline from "./UsageSparkline";

function pt(at: number, percent: number) {
  return { at, percent };
}

describe("UsageSparkline", () => {
  it("shows the hovered point's percent and time", () => {
    render(<UsageSparkline points={[pt(0, 10), pt(60, 30), pt(120, 55)]} />);
    fireEvent.mouseEnter(screen.getAllByTestId("spark-hit")[1]);
    expect(screen.getByRole("tooltip")).toHaveTextContent("30%");
    fireEvent.mouseLeave(screen.getAllByTestId("spark-hit")[1]);
    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("keeps a title fallback per point and a summary aria-label", () => {
    render(<UsageSparkline points={[pt(0, 10), pt(60, 55)]} />);
    expect(screen.getByRole("img")).toHaveAttribute("aria-label", expect.stringContaining("55%"));
    expect(document.querySelectorAll("title")).toHaveLength(2);
  });

  it("keeps both hit bands when two samples share one timestamp", () => {
    // `buildTrend` sorts by `at` without deduping, so two samples in the same second are
    // legal data. Keying the bands by `at` alone makes React see a duplicate key (its
    // reconciler then reuses whichever band it likes), so each band is keyed by position.
    const warnings: string[] = [];
    const spy = vi.spyOn(console, "error").mockImplementation((...args: unknown[]) => {
      warnings.push(args.map((a) => String(a)).join(" "));
    });
    try {
      render(<UsageSparkline points={[pt(60, 20), pt(60, 45), pt(120, 55)]} />);
      const bands = screen.getAllByTestId("spark-hit");
      expect(bands).toHaveLength(3);
      expect(warnings.filter((line) => line.includes("same key"))).toEqual([]);
      fireEvent.mouseEnter(bands[1]);
      expect(screen.getByRole("tooltip")).toHaveTextContent("45%");
      fireEvent.mouseEnter(bands[0]);
      expect(screen.getByRole("tooltip")).toHaveTextContent("20%");
    } finally {
      spy.mockRestore();
    }
  });

  it("renders nothing below two points", () => {
    expect(render(<UsageSparkline points={[pt(0, 10)]} />).container).toBeEmptyDOMElement();
  });

  it("names latest, min and max in the summary label", () => {
    render(<UsageSparkline points={[pt(0, 41), pt(60, 12), pt(120, 55)]} />);
    const label = screen.getByRole("img").getAttribute("aria-label") ?? "";
    expect(label).toContain("最新 55%");
    expect(label).toContain("最低 12%");
    expect(label).toContain("最高 55%");
  });

  it("inspects points from the keyboard", () => {
    render(<UsageSparkline points={[pt(0, 10), pt(60, 30), pt(120, 55)]} />);
    const row = screen.getByTestId("spark-keyboard");
    fireEvent.focus(row);
    // Focusing lands on the newest sample: that is the one people open the panel for.
    expect(screen.getByRole("tooltip")).toHaveTextContent("55%");
    fireEvent.keyDown(row, { key: "ArrowLeft" });
    expect(screen.getByRole("tooltip")).toHaveTextContent("30%");
    fireEvent.keyDown(row, { key: "ArrowLeft" });
    expect(screen.getByRole("tooltip")).toHaveTextContent("10%");
    // Clamped at the oldest sample instead of wrapping to the newest.
    fireEvent.keyDown(row, { key: "ArrowLeft" });
    expect(screen.getByRole("tooltip")).toHaveTextContent("10%");
    fireEvent.keyDown(row, { key: "ArrowRight" });
    expect(screen.getByRole("tooltip")).toHaveTextContent("30%");
    fireEvent.keyDown(row, { key: "Escape" });
    expect(screen.queryByRole("tooltip")).toBeNull();
    fireEvent.focus(row);
    fireEvent.blur(row);
    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("announces the inspected point through a live region", () => {
    render(<UsageSparkline points={[pt(0, 10), pt(60, 30)]} />);
    const live = screen.getByTestId("spark-live");
    expect(live).toHaveAttribute("aria-live", "polite");
    // Idle: the live region repeats the summary instead of going silent.
    expect(live.textContent).toContain("最新 30%");
    fireEvent.focus(screen.getByTestId("spark-keyboard"));
    expect(live.textContent).toContain("30%");
  });

  it("downsamples a dense series while keeping the peak", () => {
    // 200 samples in 120 px is ~0.6 px per sample: drawing one vertex each would make every
    // spike invisible, so the chart buckets and keeps each bucket's min, median and max.
    const samples = Array.from({ length: 200 }, (_, i) => pt(i, i === 137 ? 99 : 10));
    const { container } = render(<UsageSparkline points={samples} width={120} />);
    const d = container.querySelector("path")?.getAttribute("d") ?? "";
    // 15 buckets x up to 3 vertices = 45 points; the bound is 72, NOT `MAX_BUCKETS` — at this
    // width the density floor (span 14) already beats the cap (span 9), and what changed here
    // is vertices-per-bucket (2 → 3). Do not tighten it back on the assumption the cap binds.
    expect(d.split(" ").length - 1).toBeLessThanOrEqual(72);
    const peak = `${(137 * (120 / 199)).toFixed(1)},${(16 - 1 - (99 / 100) * 14).toFixed(1)}`;
    expect(d).toContain(peak);
    // One band per bucket, not per sample: fewer targets than points.
    expect(screen.getAllByTestId("spark-hit").length).toBeLessThan(200);
  });

  it("draws the median vertex between the min and the max", () => {
    // 40 samples in 120 px → buckets of 3. Each bucket is [90, 50, 10], so min/max alone give
    // two distinct y-levels and the 50 only reaches the path through the median vertex.
    const points = Array.from({ length: 40 }, (_, i) => ({
      percent: i % 3 === 0 ? 90 : i % 3 === 2 ? 10 : 50,
      at: i,
      label: `w${i}`,
    }));
    const { container } = render(<UsageSparkline points={points} kind="daily" />);
    const d = container.querySelector("path")?.getAttribute("d") ?? "";
    // y is the second coordinate of every `M{x},{y}` / `L{x},{y}` pair.
    expect(new Set(d.match(/,\d+\.\d+/g) ?? []).size).toBe(3);
  });

  it("names the sample range when a bucket is inspected", () => {
    const samples = Array.from({ length: 200 }, (_, i) => pt(i, 20 + (i % 7)));
    render(<UsageSparkline points={samples} width={120} />);
    const row = screen.getByTestId("spark-keyboard");
    fireEvent.focus(row);
    const tip = screen.getByRole("tooltip").textContent ?? "";
    expect(tip).toMatch(/样本 \d+–\d+/);
    const before = tip;
    fireEvent.keyDown(row, { key: "ArrowLeft" });
    expect(screen.getByRole("tooltip").textContent).not.toBe(before);
  });

  it("renders nothing at zero width", () => {
    const { container } = render(<UsageSparkline points={[pt(0, 10), pt(60, 30)]} width={0} />);
    expect(container).toBeEmptyDOMElement();
  });
});
