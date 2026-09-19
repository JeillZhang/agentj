// agentj-ui/apps/web/src/components/dialogResize.test.ts
import { describe, expect, it } from "vitest";
import {
  DIALOG_MIN_HEIGHT,
  DIALOG_MIN_WIDTH,
  DIALOG_VIEWPORT_MARGIN,
  RESIZE_HANDLES,
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

  it("caps the size at the viewport and keeps the rect inside", () => {
    const capped = resizeRect(START, "se", 5000, 5000, VIEWPORT);
    expect(capped.width).toBe(VIEWPORT.width - 2 * MARGIN);
    expect(capped.height).toBe(VIEWPORT.height - 2 * MARGIN);
    expect(capped.left).toBe(MARGIN);
    expect(capped.top).toBe(MARGIN);
  });

  it("never leaves the rect outside the viewport margins", () => {
    const rect = resizeRect({ left: 1000, top: 740, width: 400, height: 300 }, "e", 100, 100, VIEWPORT);
    expect(rect.left).toBeGreaterThanOrEqual(MARGIN);
    expect(rect.top).toBeGreaterThanOrEqual(MARGIN);
    expect(rect.left + rect.width).toBeLessThanOrEqual(VIEWPORT.width - MARGIN);
    expect(rect.top + rect.height).toBeLessThanOrEqual(VIEWPORT.height - MARGIN);
  });
});
