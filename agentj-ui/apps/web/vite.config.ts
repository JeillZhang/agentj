import path from "node:path";
import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import { versionSriPlugin } from "./build/version-sri-plugin.ts";
export default defineConfig({
  plugins: [react(), versionSriPlugin()],
  resolve: {
    alias: {
      "@agentj/sdk": path.resolve(
        import.meta.dirname,
        "../../packages/sdk/src/index.ts",
      ),
    },
  },
  test: {
    environment: "jsdom",
    globals: true,
    include: ["src/**/*.test.{ts,tsx}", "build/**/*.test.ts"],
  },
});
