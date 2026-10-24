import { describe, expect, it } from "vitest";
import { isBlankText } from "../blankText.js";

describe("isBlankText (BUG-017)", () => {
  it("treats the empty string and plain whitespace as blank", () => {
    for (const s of ["", " ", "\n", "\n\n", "\t ", " \r\n\t"]) {
      expect(isBlankText(s), JSON.stringify(s)).toBe(true);
    }
  });

  it("treats zero-width and formatting code points as blank", () => {
    // U+200B is the one real gateways stream as a keep-alive delta: it has no
    // Unicode White_Space property, so String.trim() does NOT remove it.
    for (const s of [
      "\u200b",
      "\u200b\u200b\u200b",
      "\u200c",
      "\u200d",
      "\u200e",
      "\u200f",
      "\u2060",
      "\ufeff",
      " \u200b\n\u200b\t",
    ]) {
      expect(isBlankText(s), JSON.stringify(s)).toBe(true);
    }
  });

  it("keeps anything with a visible glyph, even when wrapped in zero-widths", () => {
    for (const s of ["a", "分析一下当前项目的优缺点。", "\u200b分析", "分析\u200b", "a b", "0", "-"]) {
      expect(isBlankText(s), JSON.stringify(s)).toBe(false);
    }
  });

  it("does not treat trim()-visible-but-invisible-only text as content twice", () => {
    // Guard against regressing to a plain trim() implementation.
    expect("\u200b".trim()).toBe("\u200b");
    expect(isBlankText("\u200b")).toBe(true);
  });
});
