import { describe, expect, test } from "vitest";
import { formatContextWindow, modelKey, resolveModelMarker } from "../modelMeta.js";

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

type Entry = { provider?: string | null; name: string };
const e = (provider: string, name: string): Entry => ({ provider, name });

describe("modelKey", () => {
  test("keeps the NUL separator so equal names under two providers differ", () => {
    expect(modelKey(e("a", "x"))).not.toEqual(modelKey(e("b", "x")));
    expect(modelKey(e("a", "x"))).toEqual(modelKey({ name: "x", provider: "a" }));
  });
});

describe("resolveModelMarker", () => {
  test("resolves an explicit provider-qualified pick", () => {
    const providers = [e("opencode-go", "glm-5"), e("mock", "glm-5")];
    expect(resolveModelMarker(providers, e("mock", "glm-5"), "glm-5")).toEqual({
      index: 1,
      match: "explicit",
    });
  });

  test("marks nothing for an ambiguous default", () => {
    expect(resolveModelMarker([e("a", "glm-5"), e("b", "glm-5")], null, "glm-5")).toEqual({
      index: -1,
      match: "none",
    });
  });

  test("marks a unique default", () => {
    expect(resolveModelMarker([e("a", "x"), e("a", "y")], null, "y")).toEqual({
      index: 1,
      match: "default",
    });
  });

  test("marks nothing when no row matches at all", () => {
    expect(resolveModelMarker([e("a", "x")], e("a", "gone"), "gone")).toEqual({
      index: -1,
      match: "none",
    });
    expect(resolveModelMarker([e("a", "x")], null, null)).toEqual({ index: -1, match: "none" });
  });
});
