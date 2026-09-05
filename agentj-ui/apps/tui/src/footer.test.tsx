import { describe, expect, test } from "bun:test";
import { formatUsage } from "./runview.js";

describe("formatUsage", () => {
  test("usage footer formats token counts", () => {
    expect(formatUsage({ input_tokens: 10, output_tokens: 5 })).toBe("in=10 out=5");
  });

  test("usage footer handles missing usage", () => {
    expect(formatUsage(undefined)).toBe("usage: n/a");
  });
});
