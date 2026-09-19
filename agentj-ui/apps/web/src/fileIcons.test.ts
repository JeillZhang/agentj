import "@testing-library/jest-dom/vitest";
import { describe, expect, test } from "vitest";
import { extensionOf, fileIconFor } from "./fileIcons";
import type { FileCategory } from "./fileIcons";

describe("extensionOf", () => {
  test("returns the lowercased extension after the last dot", () => {
    expect(extensionOf("App.TSX")).toBe("tsx");
    expect(extensionOf("archive.tar.gz")).toBe("gz");
    expect(extensionOf("README.md")).toBe("md");
  });

  test("handles dotfiles, dangling dots and extensionless names", () => {
    expect(extensionOf(".env")).toBe("env");
    expect(extensionOf("Makefile")).toBe("");
    expect(extensionOf("trailing.")).toBe("");
  });
});

describe("fileIconFor", () => {
  test.each<[string, FileCategory]>([
    ["App.tsx", "code"],
    ["index.mjs", "code"],
    ["widget.VUE", "code"],
    ["tsconfig.json", "json"],
    ["settings.jsonc", "json"],
    ["README.md", "doc"],
    ["notes.mdx", "doc"],
    ["styles.scss", "style"],
    ["Cargo.toml", "config"],
    ["ci.yml", "config"],
    [".env", "config"],
    ["deploy.sh", "shell"],
    ["logo.PNG", "image"],
    ["icon.svg", "image"],
    ["bundle.tar.gz", "archive"],
    ["pnpm-lock.lock", "archive"],
  ])("maps %s to category %s", (name, category) => {
    expect(fileIconFor(name).category).toBe(category);
  });

  test("is case-insensitive and ignores the surrounding path", () => {
    expect(fileIconFor("src/components/App.TSX").category).toBe(fileIconFor("app.tsx").category);
  });

  test("falls back to default for unknown extensions and dotless names", () => {
    expect(fileIconFor("Makefile").category).toBe("default");
    expect(fileIconFor("data.xyz").category).toBe("default");
    expect(fileIconFor("LICENSE").category).toBe("default");
  });

  test("returns a renderable icon component for every category", () => {
    for (const name of ["a.ts", "b.json", "c.md", "d.css", "e.toml", "f.sh", "g.png", "h.zip", "i.unknown"]) {
      expect(typeof fileIconFor(name).Icon).toBe("object");
    }
  });
});
