// agentj-ui/apps/web/build/version-sri-plugin.test.ts
import { describe, expect, it } from "vitest";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { versionSriPlugin } from "./version-sri-plugin";

type BundleItem = { type: "chunk" | "asset"; fileName: string; isDynamicEntry?: boolean };
type WriteBundleHook = (
  this: unknown,
  options: { dir: string; root: string },
  bundle: Record<string, BundleItem>,
) => void | Promise<void>;

const ENTRY_JS = "console.log(1)\n";
const LAZY_JS = "export const lazy = 1\n";
const STYLE_CSS = "body{}\n";

function makeDist(): string {
  const dir = mkdtempSync(join(tmpdir(), "sri-plugin-"));
  writeFileSync(join(dir, "index.html"), `<script type="module" src="/assets/index.js"></script>`);
  mkdirSync(join(dir, "assets"));
  writeFileSync(join(dir, "assets/index.js"), ENTRY_JS);
  writeFileSync(join(dir, "assets/chunk-lazy.js"), LAZY_JS);
  writeFileSync(join(dir, "assets/style.css"), STYLE_CSS);
  return dir;
}

async function runWriteBundle(dir: string, bundle: Record<string, BundleItem>): Promise<void> {
  const plugin = versionSriPlugin();
  const hook = plugin.writeBundle as WriteBundleHook;
  await hook.call({}, { dir, root: dir }, bundle);
}

function readManifest(dir: string): { algorithm: string; assets: Record<string, string> } {
  return JSON.parse(readFileSync(join(dir, "asset-manifest.json"), "utf8"));
}

describe("versionSriPlugin writeBundle", () => {
  it("collects bundle chunks (including dynamic) into the manifest", async () => {
    const dir = makeDist();
    const bundle = {
      "assets/index.js": { type: "chunk", fileName: "assets/index.js", isDynamicEntry: false },
      "assets/chunk-lazy.js": { type: "chunk", fileName: "assets/chunk-lazy.js", isDynamicEntry: true },
    } satisfies Record<string, BundleItem>;
    await runWriteBundle(dir, bundle);
    const manifest = readManifest(dir);
    expect(manifest.assets["/assets/chunk-lazy.js"]).toBeTruthy();
    rmSync(dir, { recursive: true, force: true });
  });

  it("lists the entry chunk, dynamic chunk and asset plus /index.html with SRI injected", async () => {
    const dir = makeDist();
    const bundle = {
      "assets/index.js": { type: "chunk", fileName: "assets/index.js", isDynamicEntry: false },
      "assets/chunk-lazy.js": { type: "chunk", fileName: "assets/chunk-lazy.js", isDynamicEntry: true },
      "assets/style.css": { type: "asset", fileName: "assets/style.css" },
    } satisfies Record<string, BundleItem>;
    await runWriteBundle(dir, bundle);
    const manifest = readManifest(dir);
    expect(manifest.algorithm).toBe("sha384");
    expect(Object.keys(manifest.assets).sort()).toEqual([
      "/assets/chunk-lazy.js",
      "/assets/index.js",
      "/assets/style.css",
      "/index.html",
    ]);
    const html = readFileSync(join(dir, "index.html"), "utf8");
    expect(html).toContain(`integrity="${manifest.assets["/assets/index.js"]}"`);
    expect(manifest.assets["/index.html"]).toMatch(/^sha384-/);
    rmSync(dir, { recursive: true, force: true });
  });

  it("keeps version.json and asset-manifest.json out of the manifest", async () => {
    const dir = makeDist();
    writeFileSync(join(dir, "version.json"), "{}");
    writeFileSync(join(dir, "asset-manifest.json"), "{}");
    const bundle = {
      "assets/index.js": { type: "chunk", fileName: "assets/index.js", isDynamicEntry: false },
      "version.json": { type: "asset", fileName: "version.json" },
      "asset-manifest.json": { type: "asset", fileName: "asset-manifest.json" },
    } satisfies Record<string, BundleItem>;
    await runWriteBundle(dir, bundle);
    const manifest = readManifest(dir);
    expect(Object.keys(manifest.assets).sort()).toEqual(["/assets/index.js", "/index.html"]);
    rmSync(dir, { recursive: true, force: true });
  });
});
