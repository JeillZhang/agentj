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

function readTokensCss(): string {
  const candidates = [
    // cwd = apps/web during vitest; also cover repo-root / agentj-ui invocations
    join(process.cwd(), "..", "..", "packages/tokens/tokens.css"),
    join(process.cwd(), "packages/tokens/tokens.css"),
    join(process.cwd(), "agentj-ui/packages/tokens/tokens.css"),
    "agentj-ui/packages/tokens/tokens.css",
  ];
  for (const p of candidates) {
    if (existsSync(p)) return readFileSync(p, "utf8");
  }
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
    expect(indexCss).not.toContain("--aj-radius-ctl");
    expect(indexCss).not.toContain("--aj-radius-card");
    expect(indexCss).toContain("cubic-bezier(0.4,0,0.2,1)");
    const tokens = readTokensCss();
    expect(tokens).toContain("--aj-radiusCtl:8px");
    expect(tokens).toContain("--aj-radiusCard:12px");
  });
  it("keeps layout constants single-sourced in tokens.css", () => {
    expect(indexCss).not.toMatch(/--aj-gap:\s*12px/);
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

describe("T0 sidebar width and density vars", () => {
  it("defines --aj-sidebar-width and shadow and bg-diff", () => {
    expect(indexCss).toMatch(/--aj-sidebar-width:\s*256px/);
    expect(indexCss).toMatch(/--aj-shadow-soft-sm:\s*0 4px 16px rgba\(0,0,0,\.06\)/);
    expect(indexCss).toMatch(/--aj-sidebar-bg-diff:\s*color-mix/);
  });
  it("density vars present", () => {
    expect(readTokensCss()).toMatch(/--aj-gap:\s*12px/);
    expect(indexCss).toMatch(/--aj-shadow-soft-sm/);
  });
});
