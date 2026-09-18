import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const SEMANTIC_KEYS = [
  "bg","sidebar","panel","raised","text","textMuted","accent","accentOnAccent",
  "userBubble","border","codeBg","inlineCodeBg","success","error","warning",
  "shadowSoft","shadowRaised","radiusXl","textCaption",
  "radiusCtl","radiusCard","accentTint","diffAddTint","diffDelTint",
] as const;
export type SemanticKey = (typeof SEMANTIC_KEYS)[number];
export type ThemeName = "light" | "dark";
export type Palette = Record<SemanticKey, string>;

const deepsuite = {
  light: {
    bg: "#FFFFFF", sidebar: "#F9FAFB", panel: "#FFFFFF", raised: "#E9EDF1",
    text: "#0F1115", textMuted: "#61666B", accent: "#4176E6",
    accentOnAccent: "#FFFFFF", userBubble: "#EAF1FE", border: "#CBD2DB",
    codeBg: "#F9FAFB", inlineCodeBg: "#FAFAFA", success: "#22C55E",
    error: "#EC1313", warning: "#F59E0B",
    shadowSoft: "0 8px 24px rgba(0,0,0,.08)",
    shadowRaised: "0 12px 32px rgba(0,0,0,.12)",
    radiusXl: "22px", textCaption: "#8A8F98",
    radiusCtl: "8px", radiusCard: "12px", accentTint: "rgba(65,118,230,.10)",
    diffAddTint: "rgba(34,197,94,.14)", diffDelTint: "rgba(236,19,19,.10)",
  },
  dark: {
    bg: "#151517", sidebar: "#1B1B1C", panel: "#232324", raised: "#2C2C2E",
    text: "#F9FAFB", textMuted: "#CFD3D6", accent: "#679EFE",
    accentOnAccent: "#0A0C12", userBubble: "#2C2C2E", border: "#2E2E31",
    codeBg: "#1B1B1C", inlineCodeBg: "#292929", success: "#22C55E",
    error: "#F25A5A", warning: "#F59E0B",
    shadowSoft: "0 8px 24px rgba(0,0,0,.08)",
    shadowRaised: "0 12px 32px rgba(0,0,0,.12)",
    radiusXl: "22px", textCaption: "#8B8F98",
    radiusCtl: "8px", radiusCard: "12px", accentTint: "rgba(103,158,254,.12)",
    diffAddTint: "rgba(34,197,94,.12)", diffDelTint: "rgba(242,90,90,.12)",
  },
} satisfies Record<ThemeName, Record<SemanticKey, string>>;

export const palettes: Record<ThemeName, Palette> = deepsuite;

export const LAYOUT = {
  leftWidth: "256px", leftMin: "200px", leftMax: "420px",
  rightWidth: "320px", rightMin: "260px", rightMax: "480px",
  splitterHit: "7px", topbarH: "48px", gap: "12px",
} as const;

const varName = (key: SemanticKey) => `--aj-${key}`;
export const cssForMode = (mode: ThemeName, p: Palette): string =>
  SEMANTIC_KEYS.map((k) => `${varName(k)}:${p[k]}`).join(";") + ";";

export function tokensCss(): string {
  const lightVars = cssForMode("light", palettes.light);
  const layoutVars = Object.entries(LAYOUT)
    .map(([k, v]) => `--aj-${k}:${v}`)
    .join(";") + ";";
  const darkVars = cssForMode("dark", palettes.dark);
  const darkBlock = `@media (prefers-color-scheme: dark) { :root { ${darkVars} } }\n` +
    `:root[data-theme="dark"] { ${darkVars} }\n` +
    `:root[data-theme="light"] { ${lightVars} }\n`;
  return `:root { ${lightVars}${layoutVars} }\n${darkBlock}`;
}

export function tokensJson(): string {
  return JSON.stringify({ ...palettes, layout: LAYOUT }, null, 2) + "\n";
}

export function tokensMarkdown(): string {
  const rows = SEMANTIC_KEYS.map(
    (k) => `| ${k} | ${palettes.light[k]} | ${palettes.dark[k]} |`,
  ).join("\n");
  const layoutRows = Object.entries(LAYOUT)
    .map(([k, v]) => `| ${k} | ${v} |`)
    .join("\n");
  const head = "# agentj UI tokens\n\nSingle source: " +
    "`agentj-ui/packages/tokens/src/palette.ts`. CSS: `tokens.css` (Web). " +
    "JSON: `tokens.json` (TUI).\n\n";
  return `${head}| token | light | dark |\n|---|---|---|\n${rows}\n\n` +
    `## layout\n\n| key | value |\n|---|---|\n${layoutRows}\n`;
}

export function writeArtifacts(): void {
  const here = dirname(fileURLToPath(import.meta.url));
  const root = join(here, "..");
  writeFileSync(join(root, "tokens.css"), tokensCss());
  writeFileSync(join(root, "tokens.json"), tokensJson());
  mkdirSync(join(root, "docs"), { recursive: true });
  writeFileSync(join(root, "docs", "ui-tokens.md"), tokensMarkdown());
}
