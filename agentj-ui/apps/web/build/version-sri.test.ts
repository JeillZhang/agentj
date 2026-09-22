// agentj-ui/apps/web/build/version-sri.test.ts
import { describe, expect, it } from "vitest";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { buildVersionJson, finalizeDist, injectSri } from "./version-sri";

const APP_JS = "console.log(1)\n";
const EXPECTED = "sha384-M203vLCvZdI2DAFEpVs3YNPxNFNtwrWrHY9XrXkEzOjLmndNNAWAdMk0uRYc2i2r";

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
