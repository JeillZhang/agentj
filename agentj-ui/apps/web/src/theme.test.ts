import { describe, expect, it } from "vitest";
import { readFileSync, existsSync } from "node:fs";
import { join } from "node:path";

function readCss(name: string): string {
  const candidates = [
    // plan's original resolution
    (() => {
      try {
        return join(new URL(".", import.meta.url).pathname, name);
      } catch {
        return "";
      }
    })(),
    // vite/jsdom fallback: cwd is apps/web, or repo root, or agentj-ui
    join(process.cwd(), "src", name),
    join(process.cwd(), "apps/web/src", name),
    join(process.cwd(), "agentj-ui/apps/web/src", name),
    ...(import.meta.dirname ? [join(import.meta.dirname as string, name)] : []),
  ].filter(Boolean);
  for (const p of candidates) {
    if (existsSync(p)) return readFileSync(p, "utf8");
  }
  // last resort: throw with first candidate for diagnostics
  return readFileSync(candidates[0]!, "utf8");
}

const indexCss = readCss("index.css");
const appCss = readCss("App.css");

describe("web theme layer", () => {
  it("imports the shared tokens package css first", () => {
    const lines = indexCss.split("\n");
    expect(lines[0]).toContain('@import "@agentj/tokens/tokens.css"');
  });
  it("declares color-scheme and system dark layering", () => {
    expect(indexCss).toMatch(/color-scheme:\s*(light|dark)/i);
    expect(indexCss).toContain("@media (prefers-color-scheme: dark)");
  });
  it("pre-declares the manual-toggle override rules consumed by the T17 shell", () => {
    expect(indexCss).toContain(':root[data-theme="dark"]');
    expect(indexCss).toContain(':root[data-theme="light"]');
  });
  it("keeps the deepsuite token values and drops the purple scaffold", () => {
    expect(indexCss).toContain("#4176E6"); // light accent surfaces
    expect(indexCss).toContain("#679EFE"); // dark accent surfaces
    expect(indexCss).not.toMatch(/#aa3bff/i);
    expect(indexCss).not.toMatch(/#6b6375/i);
  });
  it("exposes radius, motion and easing design tokens", () => {
    expect(indexCss).toContain("--aj-radius-card");
    expect(indexCss).toContain("--aj-radius-ctl");
    expect(indexCss).toContain("cubic-bezier(0.4,0,0.2,1)");
  });
  it("has focus visibility and reduced-motion guards", () => {
    expect(indexCss).toContain(":focus-visible");
    expect(indexCss).toContain("prefers-reduced-motion");
  });
  it("App.css colors all come from tokens and scaffold selectors are gone", () => {
    const colorLines = appCss
      .split("\n")
      .filter((l) => /color|background|border/.test(l) && l.includes("#"));
    for (const l of colorLines) {
      expect(l).toMatch(/var\(--aj-/); // no raw hex colors in component styles
    }
    expect(appCss).not.toMatch(/\.counter|\.hero|#center|#next-steps/);
  });
  it("uses the defined accentOnAccent token for on-accent text", () => {
    // B-104 regression: --aj-accent-on-accent (kebab) was used in .aj-button but
    // only the palette key accentOnAccent is defined (varName = --aj-accentOnAccent).
    expect(appCss).not.toContain("--aj-accent-on-accent");
    expect(appCss).toContain("--aj-accentOnAccent");
  });
});
