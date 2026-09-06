import { describe, expect, it } from "vitest";
import { existsSync, readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

describe("T1 scaffold", () => {
  it("lucide installed and components scaffold exists", () => {
    const webRoot = join(dirname(fileURLToPath(import.meta.url)), "../..");
    const pkg = JSON.parse(readFileSync(join(webRoot, "package.json"), "utf8")) as {
      dependencies?: Record<string, string>;
    };
    expect(pkg.dependencies?.["lucide-react"]).toBeDefined();
    expect(pkg.dependencies?.["lucide-react"]).toMatch(/^\^0\./);
    const compDir = join(webRoot, "src/components");
    for (const f of [
      "layout.css",
      "connection.css",
      "session.css",
      "composer.css",
      "conversation.css",
      "trajectory.css",
      "hero.css",
      "dialog.css",
    ]) {
      expect(existsSync(join(compDir, f)), f).toBe(true);
    }
    const main = readFileSync(join(webRoot, "src/main.tsx"), "utf8");
    expect(main).toContain("components/layout.css");
  });
});
