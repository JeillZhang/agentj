// agentj-ui/apps/web/build/version-sri.ts (pure helpers, zero deps)
import { createHash } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

export type DistVersions = { productVersion: string; protocolVersion: string; buildTime: string };
export type HashMap = Record<string, string>;
export type AssetFile = { path: string; bytes: Buffer };
export type SriResult = { html: string; hashed: string[]; hashes: HashMap };

const TAG_RE = /<(script|link)\b[^>]*?(src|href)="([^"]+)"[^>]*>/g;
// version.json is covered by the version comparison; the manifest must never include itself.
const EXCLUDED_FILES = new Set(["version.json", "asset-manifest.json"]);

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

function resolveIntegrity(
  ref: string,
  readAsset: (ref: string) => Buffer | null,
  known: HashMap,
): string {
  const precomputed = known[ref];
  if (precomputed) return precomputed;
  const bytes = readAsset(ref);
  if (!bytes) throw new Error(`SRI: referenced asset is missing from dist: ${ref}`);
  const integrity = integrityOf(bytes);
  known[ref] = integrity;
  return integrity;
}

export function injectSri(
  html: string,
  readAsset: (ref: string) => Buffer | null,
  hashes?: HashMap,
): SriResult {
  const known: HashMap = { ...hashes };
  const hashed: string[] = [];
  const out = html.replace(TAG_RE, (tag, _name: string, attr: string, ref: string) => {
    if (!isSriTarget(tag, attr) || !isLocalRef(ref)) return tag;
    const integrity = resolveIntegrity(ref, readAsset, known);
    hashed.push(ref);
    const stripped = tag.replace(/\s+integrity="[^"]*"/, "");
    return stripped.replace(/(\s(?:src|href)=")/, ` integrity="${integrity}"$1`);
  });
  return { html: out, hashed, hashes: known };
}

export function hashAssets(files: AssetFile[]): HashMap {
  const hashes: HashMap = {};
  for (const file of files) hashes[file.path] = integrityOf(file.bytes);
  return hashes;
}

export function buildManifestJson(hashes: HashMap): string {
  const assets: HashMap = {};
  for (const key of Object.keys(hashes).sort()) assets[key] = hashes[key] ?? "";
  return `${JSON.stringify({ algorithm: "sha384", assets }, null, 2)}\n`;
}

export function buildVersionJson(v: DistVersions): string {
  return `${JSON.stringify(
    { product_version: v.productVersion, protocol_version: v.protocolVersion, build_time: v.buildTime },
    null,
    2,
  )}\n`;
}

function collectBundleFiles(distDir: string, files: string[]): AssetFile[] {
  const out: AssetFile[] = [];
  for (const file of files) {
    const rel = file.replace(/^\/+/, "");
    if (EXCLUDED_FILES.has(rel)) continue;
    const abs = join(distDir, rel);
    if (!existsSync(abs)) throw new Error(`SRI: bundle file is missing from dist: /${rel}`);
    out.push({ path: `/${rel}`, bytes: readFileSync(abs) });
  }
  return out;
}

function readLocal(distDir: string, ref: string): Buffer | null {
  const file = join(distDir, ref.replace(/^\//, ""));
  return existsSync(file) ? readFileSync(file) : null;
}

export function finalizeDist(distDir: string, v: DistVersions, bundleFiles: string[] = []): SriResult {
  const htmlPath = join(distDir, "index.html");
  if (!existsSync(htmlPath)) throw new Error(`SRI: ${htmlPath} not found; run vite build first`);
  const html = readFileSync(htmlPath, "utf8");
  const bundleHashes = hashAssets(collectBundleFiles(distDir, bundleFiles));
  const result = injectSri(html, (ref) => readLocal(distDir, ref), bundleHashes);
  writeFileSync(htmlPath, result.html);
  const hashes: HashMap = {
    ...result.hashes,
    "/index.html": integrityOf(Buffer.from(result.html)),
  };
  writeFileSync(join(distDir, "asset-manifest.json"), buildManifestJson(hashes));
  writeFileSync(join(distDir, "version.json"), buildVersionJson(v));
  return { html: result.html, hashed: result.hashed, hashes };
}
