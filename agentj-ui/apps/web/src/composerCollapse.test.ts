import { describe, expect, it } from "vitest";
import {
  COLLAPSE_DISTANCE,
  COLLAPSE_LOCK_MS,
  EXPAND_DISTANCE,
  classifyScroll,
  nextCollapsed,
} from "./composerCollapse";

describe("classifyScroll", () => {
  it("collapses on a user upward scroll past the 48px hysteresis edge", () => {
    expect(classifyScroll({ distance: 49, direction: "up", programmatic: false })).toEqual({
      type: "scroll-away",
    });
    expect(classifyScroll({ distance: 48, direction: "up", programmatic: false })).toBeNull();
  });

  it("expands on a user downward scroll inside the 40px bottom band", () => {
    expect(classifyScroll({ distance: 39, direction: "down", programmatic: false })).toEqual({
      type: "reach-bottom",
    });
    expect(classifyScroll({ distance: 40, direction: "down", programmatic: false })).toBeNull();
  });

  it("ignores programmatic auto-follow and neutral signals", () => {
    expect(classifyScroll({ distance: 400, direction: "up", programmatic: true })).toBeNull();
    expect(classifyScroll({ distance: 400, direction: "none", programmatic: false })).toBeNull();
    expect(classifyScroll({ distance: 400, direction: "down", programmatic: false })).toBeNull();
  });

  it("exports the shared thresholds and lock window", () => {
    expect(COLLAPSE_DISTANCE).toBe(48);
    expect(EXPAND_DISTANCE).toBe(40);
    expect(COLLAPSE_LOCK_MS).toBe(260);
  });
});

describe("nextCollapsed", () => {
  const noQuestion = { hasPendingQuestion: false };
  const withQuestion = { hasPendingQuestion: true };

  it("collapses on scroll-away and is idempotent", () => {
    expect(nextCollapsed(false, { type: "scroll-away" }, noQuestion)).toBe(true);
    expect(nextCollapsed(true, { type: "scroll-away" }, noQuestion)).toBe(true);
  });

  it("never collapses while a question is pending", () => {
    expect(nextCollapsed(false, { type: "scroll-away" }, withQuestion)).toBe(false);
    expect(nextCollapsed(true, { type: "scroll-away" }, withQuestion)).toBe(true);
  });

  it("expands on reach-bottom, expand-click and question-appeared", () => {
    expect(nextCollapsed(true, { type: "reach-bottom" }, noQuestion)).toBe(false);
    expect(nextCollapsed(true, { type: "expand-click" }, noQuestion)).toBe(false);
    expect(nextCollapsed(true, { type: "question-appeared" }, noQuestion)).toBe(false);
  });
});
