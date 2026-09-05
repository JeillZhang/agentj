#!/usr/bin/env node
import { readFileSync, writeFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const methodsPath = resolve(__dirname, "../../../protocol/methods.json");
const outPath = resolve(__dirname, "src/methods.ts");

function toCamelCase(method) {
  const stripped = method.startsWith("$/") ? method.slice(2) : method;
  const parts = stripped.split("/");
  return parts
    .map((part, idx) => {
      if (idx === 0) return part;
      return part.charAt(0).toUpperCase() + part.slice(1);
    })
    .join("");
}

const raw = readFileSync(methodsPath, "utf8");
const parsed = JSON.parse(raw);
const methods = parsed.methods.map((m) => m.method);

const entries = methods.map((m) => `  ${toCamelCase(m)}: "${m}",`);

const content = `// GENERATED — do not edit manually.
// Source: protocol/methods.json
// Generator: agentj-ui/packages/sdk/generate-methods.mjs
export const METHODS = {
${entries.join("\n")}
} as const;

export type MethodName = (typeof METHODS)[keyof typeof METHODS];
`;

writeFileSync(outPath, content, "utf8");
console.log(`generated ${outPath} with ${methods.length} methods`);
