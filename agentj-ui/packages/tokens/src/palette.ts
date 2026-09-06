import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const SEMANTIC_KEYS = [
  "bg","sidebar","panel","raised","text","textMuted","accent","accentOnAccent",
  "userBubble","border","codeBg","inlineCodeBg","success","error","warning",
] as const;
export type SemanticKey = (typeof SEMANTIC_KEYS)[number];
export type ThemeName = "light" | "dark";
export type Palette = Record<SemanticKey, string>;

const deepsuite = {
  light: {
    bg: "#FFFFFF", sidebar: "#F9FAFB", panel: "#FFFFFF", raised: "#F9FAFB",
    text: "#0F1115", textMuted: "#61666B", accent: "#4176E6",
    accentOnAccent: "#FFFFFF", userBubble: "#EAF1FE", border: "#E4E7EA",
    codeBg: "#F9FAFB", inlineCodeBg: "#FAFAFA", success: "#22C55E",
    error: "#EC1313", warning: "#F59E0B",
  },
  dark: {
    bg: "#151517", sidebar: "#1B1B1C", panel: "#232324", raised: "#2C2C2E",
    text: "#F9FAFB", textMuted: "#CFD3D6", accent: "#679EFE",
    accentOnAccent: "#0A0C12", userBubble: "#2C2C2E", border: "#2E2E31",
    codeBg: "#1B1B1C", inlineCodeBg: "#292929", success: "#22C55E",
    error: "#F25A5A", warning: "#F59E0B",
  },
} satisfies Record<ThemeName, Record<SemanticKey, string>>;

export const palettes: Record<ThemeName, Palette> = deepsuite;

const varName = (key: SemanticKey) => `--aj-${key}`;
export const cssForMode = (mode: ThemeName, p: Palette): string =>
  SEMANTIC_KEYS.map((k) => `${varName(k)}:${p[k]}`).join(";") + ";";

export function tokensCss(): string {
  const lightVars = cssForMode("light", palettes.light);
  const darkVars = cssForMode("dark", palettes.dark);
  const darkBlock = `@media (prefers-color-scheme: dark) { :root { ${darkVars} } }\n` +
    `:root[data-theme="dark"] { ${darkVars} }\n` +
    `:root[data-theme="light"] { ${lightVars} }\n`;
  return `:root { ${lightVars} }\n${darkBlock}`;
}

export function tokensJson(): string {
  return JSON.stringify(palettes, null, 2) + "\n";
}

export function tokensMarkdown(): string {
  const rows = SEMANTIC_KEYS.map(
    (k) => `| ${k} | ${palettes.light[k]} | ${palettes.dark[k]} |`,
  ).join("\n");
  return `# agentj UI tokens\n\nSingle source: \`agentj-ui/packages/tokens/src/palette.ts\`. CSS: \`tokens.css\` (Web). JSON: \`tokens.json\` (TUI).\n\n| token | light | dark |\n|---|---|---|\n${rows}\n`;
}

export function writeArtifacts(): void {
  const here = dirname(fileURLToPath(import.meta.url));
  const root = join(here, "..");
  writeFileSync(join(root, "tokens.css"), tokensCss());
  writeFileSync(join(root, "tokens.json"), tokensJson());
  mkdirSync(join(root, "docs"), { recursive: true });
  writeFileSync(join(root, "docs", "ui-tokens.md"), tokensMarkdown());
}
