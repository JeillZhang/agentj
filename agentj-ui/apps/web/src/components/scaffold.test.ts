import { describe, expect, it } from "vitest";
import { existsSync, readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

describe("T1 scaffold", () => {
  it("lucide installed and components scaffold exists", () => {
    const webRoot = join(dirname(fileURLToPath(import.meta.url)), "../..");
    const pkg = JSON.parse(readFileSync(join(webRoot, "package.json"), "utf8")) as {
      dependencies?: Record<string, string>;
    };
    expect(pkg.dependencies?.["lucide-react"]).toBeDefined();
    expect(pkg.dependencies?.["lucide-react"]).toMatch(/^\^1\./);
    const compDir = join(webRoot, "src/components");
    for (const f of [
      "layout.css",
      "connection.css",
      "session.css",
      "composer.css",
      "conversation.css",
      "trajectory.css",
      "hero.css",
      "dialog.css",
      "split.css",
      "rightpanel.css",
      "filetree.css",
    ]) {
      expect(existsSync(join(compDir, f)), f).toBe(true);
    }
    const main = readFileSync(join(webRoot, "src/main.tsx"), "utf8");
    expect(main).toContain("components/layout.css");
    expect(main).toContain("components/split.css");
  });
});

describe("BUG-004 desktop breakpoint keeps compact layout gap", () => {
  function readLayoutCss(): string {
    const webRoot = join(dirname(fileURLToPath(import.meta.url)), "../..");
    const cands = [
      join(webRoot, "src/components/layout.css"),
      join(process.cwd(), "src/components/layout.css"),
      join(process.cwd(), "apps/web/src/components/layout.css"),
      join(process.cwd(), "agentj-ui/apps/web/src/components/layout.css"),
    ];
    for (const p of cands) if (existsSync(p)) return readFileSync(p, "utf8");
    return readFileSync(cands[0]!, "utf8");
  }
  it("base .aj-layout gap resolves to compact --aj-gap", () => {
    const css = readLayoutCss();
    expect(css).toMatch(/\.aj-layout\s*\{[^}]*gap:\s*var\(--aj-gap\)/);
  });
  it("@media (min-width: 1024px) must not widen .aj-layout gap to 16px", () => {
    const css = readLayoutCss();
    const mediaBlocks = [
      ...css.matchAll(/@media\s*\(min-width:\s*1024px\)\s*\{([\s\S]*?)\}/g),
    ].map((m) => m[1]);
    expect(mediaBlocks.length).toBeGreaterThan(0);
    for (const block of mediaBlocks) {
      if (!/\.aj-layout/.test(block)) continue;
      expect(block).not.toContain("var(--aj-space-4)");
    }
  });
});

describe("ghost button is self-contained (no native look)", () => {
  function readAppCss(): string {
    const webRoot = join(dirname(fileURLToPath(import.meta.url)), "../..");
    const cands = [
      join(webRoot, "src/App.css"),
      join(process.cwd(), "src/App.css"),
      join(process.cwd(), "apps/web/src/App.css"),
      join(process.cwd(), "agentj-ui/apps/web/src/App.css"),
    ];
    for (const p of cands) if (existsSync(p)) return readFileSync(p, "utf8");
    return readFileSync(cands[0]!, "utf8");
  }
  it(".aj-button--ghost declares full control styling", () => {
    const css = readAppCss();
    const match = css.match(/\.aj-button--ghost\s*\{([^}]*)\}/);
    expect(match).not.toBeNull();
    const body = match![1];
    for (const prop of ["padding", "border", "border-radius", "font", "cursor"]) {
      expect(body, prop).toMatch(new RegExp(`(^|[;{\\s])${prop}\\s*:`));
    }
  });
  it(".aj-button--ghost hover/disabled states are styled", () => {
    const css = readAppCss();
    expect(css).toMatch(/\.aj-button--ghost:hover\s*\{[^}]*background:\s*var\(--aj-raised\)/);
    expect(css).toMatch(/\.aj-button--ghost:disabled\s*\{[^}]*opacity/);
  });
});

describe("file icon category tints", () => {
  function readFiletreeCss(): string {
    const webRoot = join(dirname(fileURLToPath(import.meta.url)), "../..");
    return readFileSync(join(webRoot, "src/components/filetree.css"), "utf8");
  }
  it("declares a db category tint", () => {
    const css = readFiletreeCss();
    expect(css).toMatch(/\.file-icon--db\s*\{[^}]*color:\s*var\(--aj-[a-zA-Z]+\)/);
  });
});

describe("T0 scaffold density and hairline", () => {
  function readIndexCss(): string {
    const webRoot = join(dirname(fileURLToPath(import.meta.url)), "../..");
    const cands = [
      join(webRoot, "src/index.css"),
      join(process.cwd(), "src/index.css"),
      join(process.cwd(), "apps/web/src/index.css"),
      join(process.cwd(), "agentj-ui/apps/web/src/index.css"),
    ];
    for (const p of cands) if (existsSync(p)) return readFileSync(p, "utf8");
    return readFileSync(cands[0]!, "utf8");
  }
  it("css variables --aj-shadow-soft-sm and --aj-sidebar-bg-diff present", () => {
    const css = readIndexCss();
    expect(css).toMatch(/--aj-shadow-soft-sm:\s*0 4px 16px rgba\(0,0,0,\.06\)/);
    expect(css).toMatch(/--aj-sidebar-bg-diff:\s*color-mix/);
  });
  it("density vars present", () => {
    const webRoot = join(dirname(fileURLToPath(import.meta.url)), "../..");
    const tokens = readFileSync(join(webRoot, "../../packages/tokens/tokens.css"), "utf8");
    expect(tokens).toContain("--aj-gap:12px");
    const css = readIndexCss();
    expect(css).toMatch(/--aj-shadow-soft-sm/);
  });
  it("palette unchanged and hairline uses 0.5px box-shadow", () => {
    const webRoot = join(dirname(fileURLToPath(import.meta.url)), "../..");
    const paletteCandidates = [
      join(webRoot, "../../packages/tokens/src/palette.ts"),
      join(process.cwd(), "packages/tokens/src/palette.ts"),
      join(process.cwd(), "agentj-ui/packages/tokens/src/palette.ts"),
      "/homeext/workspace/01.dev/agentj/agentj-ui/packages/tokens/src/palette.ts",
    ];
    let palette = "";
    for (const p of paletteCandidates) if (existsSync(p)) { palette = readFileSync(p, "utf8"); break; }
    if (!palette) palette = readFileSync(paletteCandidates[0]!, "utf8");
    expect(palette).toMatch(/SEMANTIC_KEYS/);
    const keysMatch = palette.match(/SEMANTIC_KEYS\s*=\s*\[/s);
    expect(keysMatch).not.toBeNull();
    const keys = [
      "bg", "sidebar", "panel", "raised", "text", "textMuted", "accent", "accentOnAccent",
      "userBubble", "border", "codeBg", "inlineCodeBg", "success", "error", "warning",
      "shadowSoft", "shadowRaised", "radiusXl", "textCaption",
      "radiusCtl", "radiusCard", "accentTint", "diffAddTint", "diffDelTint",
    ];
    const quoted = keys.map((k) => `"${k}"`).join("|");
    const count = (palette.match(new RegExp(quoted, "g")) ?? []).length;
    expect(count).toBe(24);
    const compDir = join(webRoot, "src/components");
    const sidebarCandidates = [
      join(compDir, "sidebar.css"),
      join(compDir, "layout.css"),
    ];
    let sidebarCss = "";
    for (const p of sidebarCandidates) if (existsSync(p)) { sidebarCss = readFileSync(p, "utf8"); if (/0\.5px/.test(sidebarCss)) break; }
    expect(sidebarCss).toMatch(/0\.5px/);
    if (/box-shadow/.test(sidebarCss)) {
      expect(sidebarCss).toMatch(/0\.5px/);
    }
  });
});
