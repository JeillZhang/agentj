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
});
