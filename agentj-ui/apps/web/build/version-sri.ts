// agentj-ui/apps/web/build/version-sri.ts (new; pure helpers, zero deps)
import { createHash } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

export type DistVersions = { productVersion: string; protocolVersion: string; buildTime: string };
export type SriResult = { html: string; hashed: string[] };

const TAG_RE = /<(script|link)\b[^>]*?(src|href)="([^"]+)"[^>]*>/g;

function isSriTarget(tag: string, attr: string): boolean {
  if (tag.startsWith("<script")) return attr === "src";
  return /rel="(stylesheet|modulepreload|preload)"/.test(tag);
}

function isLocalRef(ref: string): boolean {
  return ref.startsWith("/") && !ref.startsWith("//");
}

function integrityOf(bytes: Buffer): string {
  return `sha384-${createHash("sha384").update(bytes).digest("base64")}`;
}

export function injectSri(html: string, readAsset: (ref: string) => Buffer | null): SriResult {
  const hashed: string[] = [];
  const out = html.replace(TAG_RE, (tag, _name: string, attr: string, ref: string) => {
    if (!isSriTarget(tag, attr) || !isLocalRef(ref)) return tag;
    const bytes = readAsset(ref);
    if (!bytes) throw new Error(`SRI: referenced asset is missing from dist: ${ref}`);
    const integrity = integrityOf(bytes);
    hashed.push(ref);
    const stripped = tag.replace(/\s+integrity="[^"]*"/, "");
    return stripped.replace(/(\s(?:src|href)=")/, ` integrity="${integrity}"$1`);
  });
  return { html: out, hashed };
}

export function buildVersionJson(v: DistVersions): string {
  return `${JSON.stringify(
    { product_version: v.productVersion, protocol_version: v.protocolVersion, build_time: v.buildTime },
    null,
    2,
  )}\n`;
}

export function finalizeDist(distDir: string, v: DistVersions): SriResult {
  const htmlPath = join(distDir, "index.html");
  if (!existsSync(htmlPath)) throw new Error(`SRI: ${htmlPath} not found; run vite build first`);
  const html = readFileSync(htmlPath, "utf8");
  const result = injectSri(html, (ref) => {
    const file = join(distDir, ref.replace(/^\//, ""));
    return existsSync(file) ? readFileSync(file) : null;
  });
  writeFileSync(htmlPath, result.html);
  writeFileSync(join(distDir, "version.json"), buildVersionJson(v));
  return result;
}
