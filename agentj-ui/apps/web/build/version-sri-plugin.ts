import { resolve } from "node:path";
import type { Plugin } from "vite";
// Single version source of truth; tsconfig.node enables allowImportingTsExtensions.
import { PROTOCOL_VERSION, VERSION } from "../../../packages/sdk/src/config.ts";
import { finalizeDist } from "./version-sri.ts";

// Mirrors version-sri.ts: the manifest covers bundle products only, never itself or the version fact.
const EXCLUDED_BUNDLE_FILES = new Set(["version.json", "asset-manifest.json"]);

export function versionSriPlugin(): Plugin {
  let root = process.cwd();
  return {
    name: "agentj:version-sri",
    apply: "build",
    configResolved(config) {
      root = config.root;
    },
    writeBundle(options, bundle) {
      const distDir = resolve(root, options.dir ?? "dist");
      const files = Object.values(bundle)
        .map((item) => item.fileName)
        .filter((name) => !EXCLUDED_BUNDLE_FILES.has(name));
      finalizeDist(
        distDir,
        {
          productVersion: VERSION,
          protocolVersion: PROTOCOL_VERSION,
          buildTime: new Date().toISOString(),
        },
        files,
      );
    },
  };
}
