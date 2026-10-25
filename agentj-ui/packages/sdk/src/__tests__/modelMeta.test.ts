import { describe, expect, test } from "vitest";
import { formatContextWindow } from "../modelMeta.js";

describe("formatContextWindow", () => {
  test("formats a window compactly and never invents one", () => {
    expect(formatContextWindow(204_800)).toBe("205k");
    expect(formatContextWindow(1_048_576)).toBe("1.0M");
    expect(formatContextWindow(900)).toBe("900");
    expect(formatContextWindow(null)).toBeNull();
    expect(formatContextWindow(undefined)).toBeNull();
    expect(formatContextWindow(0)).toBeNull();
  });
});
