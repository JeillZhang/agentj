import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { palettes, SEMANTIC_KEYS, cssForMode, type ThemeName } from "./palette.ts";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const css = readFileSync(join(root, "tokens.css"), "utf8");
const json = readFileSync(join(root, "tokens.json"), "utf8");

describe("@agentj/tokens palette", () => {
  it("exposes exactly the bound semantic keys in both modes", () => {
    const expected = [
      "bg","sidebar","panel","raised","text","textMuted","accent","accentOnAccent",
      "userBubble","border","codeBg","inlineCodeBg","success","error","warning",
      "shadowSoft","shadowRaised","radiusXl","textCaption",
    ];
    expect(SEMANTIC_KEYS).toEqual(expected);
    for (const mode of ["light", "dark"] as ThemeName[]) {
      expect(Object.keys(palettes[mode]).sort()).toEqual([...expected].sort());
    }
  });
  it("contains shadowSoft/shadowRaised/radiusXl/textCaption with correct values", () => {
    expect(SEMANTIC_KEYS).toContain("shadowSoft");
    expect(SEMANTIC_KEYS).toContain("shadowRaised");
    expect(SEMANTIC_KEYS).toContain("radiusXl");
    expect(SEMANTIC_KEYS).toContain("textCaption");
    expect(palettes.light.shadowSoft).toMatch(/rgba/);
    expect(palettes.dark.textCaption).toMatch(/^#/);
    expect(css).toContain("--aj-shadowSoft");
    expect(css).toContain("--aj-radiusXl");
    expect(css).toContain("--aj-textCaption");
    const parsed = JSON.parse(json);
    expect(parsed.light.radiusXl).toBe("22px");
  });
  it("index.css defines spacing/font/layout/motion/icon vars", () => {
    const idx = readFileSync(join(root, "../../apps/web/src/index.css"), "utf8");
    expect(idx).toContain("--aj-space-3: 12px");
    expect(idx).toContain("--aj-content-max: 960px");
    expect(idx).toContain("--aj-z-dialog: 100");
    expect(idx).toContain("--aj-line-height: 1.6");
    expect(idx).toContain("--aj-icon-md");
  });
  it("holds the spec-bound hex values (D2/D8 table)", () => {
    expect(palettes.light.accent).toBe("#4176E6");
    expect(palettes.dark.accent).toBe("#679EFE");
    expect(palettes.light.bg).toBe("#FFFFFF");
    expect(palettes.dark.bg).toBe("#151517");
    expect(palettes.light.userBubble).toBe("#EAF1FE");
    expect(palettes.dark.userBubble).toBe("#2C2C2E");
    expect(palettes.light.error).toBe("#EC1313");
    expect(palettes.dark.error).toBe("#F25A5A");
    expect(palettes.light.border).toBe("#E4E7EA");
    expect(palettes.dark.border).toBe("#2E2E31");
  });
  it("agrees with tokens.css custom properties", () => {
    for (const mode of ["light", "dark"] as ThemeName[]) {
      for (const key of SEMANTIC_KEYS) {
        expect(css).toContain(`--aj-${mode === "light" ? key : key}:${palettes[mode][key]}`);
      }
    }
  });
  it("layers dark over media and manual attribute overrides", () => {
    expect(css).toContain("@media (prefers-color-scheme: dark)");
    expect(css).toContain('[data-theme="dark"]');
    expect(css).toContain('[data-theme="light"]');
    expect(css.indexOf('[data-theme="light"]')).toBeGreaterThan(css.indexOf("@media"));
  });
  it("agrees with tokens.json", () => {
    const parsed = JSON.parse(json);
    for (const mode of ["light", "dark"] as ThemeName[]) {
      const jm = parsed[mode];
      for (const key of SEMANTIC_KEYS) {
        expect(jm[key]).toBe(palettes[mode][key]);
      }
    }
  });
  it("cssForMode emits one custom property per key", () => {
    const out = cssForMode("dark", palettes.dark);
    expect(out.split(";").length).toBe(SEMANTIC_KEYS.length + 1); // trailing newline
  });
});
