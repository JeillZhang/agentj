// agentj-ui/apps/web/src/components/dialogResize.test.ts
import { describe, expect, it } from "vitest";
import {
  DIALOG_MIN_HEIGHT,
  DIALOG_MIN_WIDTH,
  DIALOG_VIEWPORT_MARGIN,
  RESIZE_HANDLES,
  clampDialogRect,
  resizeRect,
} from "./dialogResize";
import type { DialogRect } from "./dialogResize";

const VIEWPORT = { width: 1024, height: 768 };
const START: DialogRect = { left: 200, top: 150, width: 560, height: 400 };
const MARGIN = DIALOG_VIEWPORT_MARGIN;

describe("resizeRect", () => {
  it("exposes the eight edge and corner handles", () => {
    expect([...RESIZE_HANDLES].sort()).toEqual(["e", "n", "ne", "nw", "s", "se", "sw", "w"]);
  });

  it("grows the east/south edges without moving the origin", () => {
    expect(resizeRect(START, "e", 40, 999, VIEWPORT)).toEqual({
      left: 200,
      top: 150,
      width: 600,
      height: 400,
    });
    expect(resizeRect(START, "s", 999, 30, VIEWPORT)).toEqual({
      left: 200,
      top: 150,
      width: 560,
      height: 430,
    });
  });

  it("anchors the opposite edge when dragging west/north", () => {
    expect(resizeRect(START, "w", -40, 0, VIEWPORT)).toEqual({
      left: 160,
      top: 150,
      width: 600,
      height: 400,
    });
    expect(resizeRect(START, "n", 0, -30, VIEWPORT)).toEqual({
      left: 200,
      top: 120,
      width: 560,
      height: 430,
    });
  });

  it("resizes both axes from a corner", () => {
    expect(resizeRect(START, "se", 40, 30, VIEWPORT)).toEqual({
      left: 200,
      top: 150,
      width: 600,
      height: 430,
    });
    expect(resizeRect(START, "nw", -40, -30, VIEWPORT)).toEqual({
      left: 160,
      top: 120,
      width: 600,
      height: 430,
    });
  });

  it("clamps to the minimum size while keeping the opposite edge anchored", () => {
    expect(resizeRect(START, "nw", 5000, 5000, VIEWPORT)).toEqual({
      left: START.left + START.width - DIALOG_MIN_WIDTH,
      top: START.top + START.height - DIALOG_MIN_HEIGHT,
      width: DIALOG_MIN_WIDTH,
      height: DIALOG_MIN_HEIGHT,
    });
  });

  it("caps growth at the remaining viewport space without moving the anchored origin", () => {
    const capped = resizeRect(START, "se", 5000, 5000, VIEWPORT);
    expect(capped).toEqual({ left: 200, top: 150, width: 816, height: 610 });
    expect(capped.left + capped.width).toBe(VIEWPORT.width - MARGIN);
    expect(capped.top + capped.height).toBe(VIEWPORT.height - MARGIN);
  });

  it("caps a per-dialog minimum at the viewport on narrow screens", () => {
    const narrow = { width: 375, height: 667 };
    const rect = resizeRect(
      { left: 16, top: 120, width: 343, height: 400 },
      "se",
      0,
      0,
      narrow,
      { minWidth: 560 },
    );
    expect(rect.width).toBe(narrow.width - 2 * MARGIN);
    expect(rect.left + rect.width).toBeLessThanOrEqual(narrow.width - MARGIN);
  });

  it("honors custom min size limits while anchoring the opposite edge", () => {
    const se = resizeRect(START, "se", -5000, -5000, VIEWPORT, { minWidth: 560, minHeight: 300 });
    expect(se).toMatchObject({ width: 560, height: 300 });
    const west = resizeRect(START, "w", 5000, 0, VIEWPORT, { minWidth: 560 });
    expect(west.width).toBe(560);
    expect(west.left).toBe(START.left + START.width - 560);
  });

  it("never leaves the rect outside the viewport margins", () => {
    const rect = resizeRect({ left: 1000, top: 740, width: 400, height: 300 }, "e", 100, 100, VIEWPORT);
    expect(rect.left).toBeGreaterThanOrEqual(MARGIN);
    expect(rect.top).toBeGreaterThanOrEqual(MARGIN);
    expect(rect.left + rect.width).toBeLessThanOrEqual(VIEWPORT.width - MARGIN);
    expect(rect.top + rect.height).toBeLessThanOrEqual(VIEWPORT.height - MARGIN);
  });
});

describe("clampDialogRect", () => {
  it("shrinks and repositions a rect into a smaller viewport", () => {
    expect(
      clampDialogRect({ left: 100, top: 100, width: 660, height: 460 }, { width: 500, height: 768 }),
    ).toEqual({ left: 8, top: 100, width: 484, height: 460 });
  });

  it("keeps per-dialog minimums when the viewport allows them", () => {
    const rect = clampDialogRect({ left: 0, top: 0, width: 100, height: 100 }, VIEWPORT, {
      minWidth: 560,
      minHeight: 300,
    });
    expect(rect).toMatchObject({ width: 560, height: 300, left: 8, top: 8 });
  });
});
