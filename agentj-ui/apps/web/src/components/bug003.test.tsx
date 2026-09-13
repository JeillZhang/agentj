import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, test } from "vitest";

// BUG-003 regression guards. The defect is a viewport/CSS layout issue that
// jsdom cannot measure (short viewports push .comp-tools + send below the fold
// and clip the upward option panels), so like a11y.test.tsx we assert the CSS
// rules that make the layout safe.

function readCss(name: string): string {
  return readFileSync(join(import.meta.dirname, name), "utf8");
}

describe("BUG-003 — short viewports keep composer toolbar + popovers visible", () => {
  test("expanded ask card may shrink & scroll inside the composer (never pushes .comp-tools out)", () => {
    const composer = readCss("composer.css");
    // The ask card is the expandable region (214px when expanded). On short
    // viewports it must be allowed to shrink and scroll inside the composer so
    // the fixed chrome (header / textarea / .comp-tools + send button) stays
    // visible instead of overflowing the composer's bottom edge.
    const rule = composer.match(/\.aj-composer-card\s*>\s*\.ask\s*\{[^}]*\}/);
    expect(rule).not.toBeNull();
    expect(rule![0]).toMatch(/min-height:\s*0/);
    expect(rule![0]).toMatch(/overflow-y:\s*auto/);
  });

  test("option popovers stay bounded + internally scrollable (fit short viewports)", () => {
    const options = readCss("options.css");
    const pop = options.match(/\.pop\s*\{[^}]*\}/);
    expect(pop).not.toBeNull();
    expect(pop![0]).toMatch(/max-height:\s*min\(420px,\s*60vh\)/);
    expect(pop![0]).toMatch(/overflow:\s*auto/);
  });
});