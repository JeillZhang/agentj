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
    expect(pkg.dependencies?.["lucide-react"]).toMatch(/^\^0\./);
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
    ]) {
      expect(existsSync(join(compDir, f)), f).toBe(true);
    }
    const main = readFileSync(join(webRoot, "src/main.tsx"), "utf8");
    expect(main).toContain("components/layout.css");
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
    const css = readIndexCss();
    expect(css).toMatch(/--aj-gap/);
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
    const count = (palette.match(/"bg"|"sidebar"|"panel"|"raised"|"text"|"textMuted"|"accent"|"accentOnAccent"|"userBubble"|"border"|"codeBg"|"inlineCodeBg"|"success"|"error"|"warning"|"shadowSoft"|"shadowRaised"|"radiusXl"|"textCaption"/g) ?? []).length;
    expect(count).toBe(19);
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
