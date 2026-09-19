// agentj-ui/apps/web/src/displayPrefsCss.test.ts
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const css = readFileSync(join(here, "components/displayPrefs.css"), "utf8");
const main = readFileSync(join(here, "main.tsx"), "utf8");

describe("displayPrefs.css override layer", () => {
  it("consumes all four zone font vars with current-value fallbacks", () => {
    expect(css).toMatch(/var\(--aj-font-conv,\s*15px\)/);
    expect(css).toMatch(/var\(--aj-font-left,\s*13px\)/);
    expect(css).toMatch(/var\(--aj-font-right,\s*13px\)/);
    expect(css).toMatch(/var\(--aj-font-code,\s*12px\)/);
    expect(css).toMatch(/--aj-lh-conv/);
  });
  it("is imported as the last component stylesheet", () => {
    const target = main.indexOf("components/displayPrefs.css");
    expect(target).toBeGreaterThan(main.indexOf("components/conversation.css"));
    expect(target).toBeGreaterThan(main.indexOf("components/dialog.css"));
    expect(target).toBeGreaterThan(main.indexOf("components/topbarmenu.css"));
  });
  it("declares density and reduced-motion hooks", () => {
    expect(css).toContain('.aj-workspace[data-density="compact"]');
    expect(css).toContain(':root[data-reduced-motion="true"]');
  });
});
