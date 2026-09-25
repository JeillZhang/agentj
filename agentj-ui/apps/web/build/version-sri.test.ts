// agentj-ui/apps/web/build/version-sri.test.ts
import { describe, expect, it } from "vitest";
import { createHash } from "node:crypto";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  buildManifestJson,
  buildVersionJson,
  finalizeDist,
  hashAssets,
  injectSri,
  integrityOf,
} from "./version-sri";

const APP_JS = "console.log(1)\n";
const EXPECTED = "sha384-M203vLCvZdI2DAFEpVs3YNPxNFNtwrWrHY9XrXkEzOjLmndNNAWAdMk0uRYc2i2r";
const PNG_BYTES = "png-bytes";
const PNG_EXPECTED = "sha384-Zr9/9I1pUeAiv6hwcUHaiFN6msCnUawgATCH+CEtwy5d1vC1Nz++Vne89D3L1Kwy";
const VERSIONS = { productVersion: "0.21.0", protocolVersion: "0.13.0", buildTime: "2026-09-22T00:00:00Z" };

it("injects sha384 integrity for local script and stylesheet refs", () => {
  const html = [
    '<script type="module" crossorigin src="/assets/app.js"></script>',
    '<link rel="stylesheet" crossorigin href="/assets/app.css">',
  ].join("\n");
  const out = injectSri(html, (ref) =>
    ref === "/assets/app.js" ? Buffer.from(APP_JS) : Buffer.from("body{}"),
  );
  expect(out.html).toContain(EXPECTED);
  expect(out.hashed.sort()).toEqual(["/assets/app.css", "/assets/app.js"]);
});

it("skips external, data and non-asset links", () => {
  const html = [
    '<script src="https://cdn/x.js"></script>',
    '<script src="//cdn/x.js"></script>',
    '<script src="data:text/javascript,1"></script>',
    '<link rel="icon" href="/favicon.svg">',
  ].join("\n");
  const out = injectSri(html, () => null);
  expect(out.html).not.toContain("integrity");
});

it("throws when a local reference is missing", () => {
  expect(() => injectSri('<script src="/assets/gone.js"></script>', () => null))
    .toThrow(/gone\.js/);
});

it("replaces an existing integrity attribute", () => {
  const html = '<script src="/assets/app.js" integrity="sha384-old"></script>';
  const out = injectSri(html, () => Buffer.from(APP_JS));
  expect(out.html.match(/integrity=/g)).toHaveLength(1);
  expect(out.html).not.toContain("sha384-old");
});

it("writes version.json and rewrites index.html in a dist directory", () => {
  const dir = mkdtempSync(join(tmpdir(), "agentj-sri-"));
  mkdirSync(join(dir, "assets"));
  writeFileSync(join(dir, "index.html"), '<script src="/assets/app.js"></script>');
  writeFileSync(join(dir, "assets/app.js"), APP_JS);
  finalizeDist(dir, { productVersion: "0.20.0", protocolVersion: "0.13.0", buildTime: "2026-09-20T00:00:00Z" });
  const v = JSON.parse(readFileSync(join(dir, "version.json"), "utf8")) as Record<string, string>;
  expect(v.product_version).toBe("0.20.0");
  expect(v.protocol_version).toBe("0.13.0");
  expect(readFileSync(join(dir, "index.html"), "utf8")).toContain(EXPECTED);
  rmSync(dir, { recursive: true, force: true });
});

it("buildVersionJson emits the documented snake_case shape", () => {
  const raw = buildVersionJson({ productVersion: "1.2.3", protocolVersion: "0.13.0", buildTime: "t" });
  expect(JSON.parse(raw)).toEqual({
    product_version: "1.2.3", protocol_version: "0.13.0", build_time: "t",
  });
});

it("hashes known bytes into sha384-base64 digests keyed by path", () => {
  const hashes = hashAssets([
    { path: "/assets/a.js", bytes: Buffer.from(APP_JS) },
    { path: "/assets/hero.png", bytes: Buffer.from(PNG_BYTES) },
  ]);
  expect(hashes).toEqual({ "/assets/a.js": EXPECTED, "/assets/hero.png": PNG_EXPECTED });
});

it("buildManifestJson emits the documented manifest shape", () => {
  const raw = buildManifestJson({ "/b.js": "sha384-b", "/a.js": "sha384-a" });
  expect(JSON.parse(raw)).toEqual({
    algorithm: "sha384",
    assets: { "/a.js": "sha384-a", "/b.js": "sha384-b" },
  });
  expect(raw.endsWith("\n")).toBe(true);
  expect(raw).toContain('\n  "algorithm": "sha384"');
});

it("writes a manifest covering bundle files and the rewritten index.html", () => {
  const dir = mkdtempSync(join(tmpdir(), "sri-manifest-"));
  writeFileSync(join(dir, "index.html"), `<script type="module" src="/assets/a.js"></script>`);
  mkdirSync(join(dir, "assets"));
  writeFileSync(join(dir, "assets/a.js"), "console.log(1)");
  writeFileSync(join(dir, "assets/hero.png"), "png-bytes");
  const result = finalizeDist(dir, VERSIONS, ["assets/a.js", "assets/hero.png"]);
  const manifest = JSON.parse(readFileSync(join(dir, "asset-manifest.json"), "utf8"));
  expect(manifest.algorithm).toBe("sha384");
  expect(Object.keys(manifest.assets).sort()).toEqual(["/assets/a.js", "/assets/hero.png", "/index.html"]);
  expect(manifest.assets["/assets/a.js"]).toBe(result.hashes["/assets/a.js"]);
  const rewritten = readFileSync(join(dir, "index.html"), "utf8");
  expect(rewritten).toContain(`integrity="${result.hashes["/assets/a.js"]}"`);
  const rewrittenDigest = `sha384-${createHash("sha384").update(rewritten).digest("base64")}`;
  expect(manifest.assets["/index.html"]).toBe(rewrittenDigest);
  rmSync(dir, { recursive: true, force: true });
});

it("excludes version.json and asset-manifest.json from the manifest", () => {
  const dir = mkdtempSync(join(tmpdir(), "sri-exclude-"));
  writeFileSync(join(dir, "index.html"), `<script src="/assets/a.js"></script>`);
  mkdirSync(join(dir, "assets"));
  writeFileSync(join(dir, "assets/a.js"), APP_JS);
  writeFileSync(join(dir, "version.json"), "{}");
  writeFileSync(join(dir, "asset-manifest.json"), "{}");
  finalizeDist(dir, VERSIONS, ["assets/a.js", "version.json", "asset-manifest.json"]);
  const manifest = JSON.parse(readFileSync(join(dir, "asset-manifest.json"), "utf8"));
  expect(Object.keys(manifest.assets).sort()).toEqual(["/assets/a.js", "/index.html"]);
  rmSync(dir, { recursive: true, force: true });
});

it("includes dist root files such as public passthrough in the manifest", () => {
  const dist = mkdtempSync(join(tmpdir(), "aj-sri-public-"));
  writeFileSync(join(dist, "index.html"), '<script src="/assets/a.js"></script>');
  mkdirSync(join(dist, "assets"), { recursive: true });
  writeFileSync(join(dist, "assets/a.js"), "console.log(1)");
  writeFileSync(join(dist, "favicon.svg"), "<svg/>");
  writeFileSync(join(dist, ".DS_Store"), "junk");
  finalizeDist(dist, { productVersion: "0.22.0", protocolVersion: "0.14.0", buildTime: "t" }, ["assets/a.js"]);
  const manifest = JSON.parse(readFileSync(join(dist, "asset-manifest.json"), "utf8"));
  expect(Object.keys(manifest.assets).sort()).toEqual(["/assets/a.js", "/favicon.svg", "/index.html"]);
  expect(manifest.assets["/favicon.svg"]).toBe(integrityOf(Buffer.from("<svg/>")));
  expect(manifest.assets["/.DS_Store"]).toBeUndefined();
  rmSync(dist, { recursive: true, force: true });
});
