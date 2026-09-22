import { resolve } from "node:path";
import type { Plugin } from "vite";
// Single version source of truth; tsconfig.node enables allowImportingTsExtensions.
import { PROTOCOL_VERSION, VERSION } from "../../../packages/sdk/src/config.ts";
import { finalizeDist } from "./version-sri.ts";

export function versionSriPlugin(): Plugin {
  let root = process.cwd();
  return {
    name: "agentj:version-sri",
    apply: "build",
    configResolved(config) {
      root = config.root;
    },
    writeBundle(options) {
      const distDir = resolve(root, options.dir ?? "dist");
      finalizeDist(distDir, {
        productVersion: VERSION,
        protocolVersion: PROTOCOL_VERSION,
        buildTime: new Date().toISOString(),
      });
    },
  };
}
