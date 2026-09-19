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
    ["main.rs", "code"],
    ["main.go", "code"],
    ["app.py", "code"],
    ["Main.java", "code"],
    ["lib.c", "code"],
    ["lib.h", "code"],
    ["lib.cpp", "code"],
    ["lib.hpp", "code"],
    ["prog.cs", "code"],
    ["app.rb", "code"],
    ["index.php", "code"],
    ["Main.kt", "code"],
    ["App.swift", "code"],
    ["app.scala", "code"],
    ["init.lua", "code"],
    ["main.dart", "code"],
    ["script.r", "code"],
    ["task.ex", "code"],
    ["core.exs", "code"],
    ["main.hs", "code"],
    ["core.clj", "code"],
    ["service.proto", "code"],
    ["schema.graphql", "code"],
    ["index.html", "code"],
    ["data.xml", "code"],
    ["schema.sql", "db"],
    ["data.db", "db"],
    ["cache.sqlite", "db"],
  ])("maps %s to category %s", (name, category) => {
    expect(fileIconFor(name).category).toBe(category);
  });

  test("maps build files by name", () => {
    for (const name of ["Dockerfile", "Makefile", "CMakeLists.txt"]) {
      expect(fileIconFor(name).category).toBe("code");
    }
  });

  test("resolves the db icon via the renderable-icon pattern", () => {
    for (const name of ["schema.sql", "data.db"]) {
      const match = fileIconFor(name);
      expect(match.category).toBe("db");
      expect(typeof match.Icon).toBe("object");
    }
  });

  test("is case-insensitive and ignores the surrounding path", () => {
    expect(fileIconFor("src/components/App.TSX").category).toBe(fileIconFor("app.tsx").category);
  });

  test("falls back to default for unknown extensions and dotless names", () => {
    expect(fileIconFor("data.xyz").category).toBe("default");
    expect(fileIconFor("LICENSE").category).toBe("default");
  });

  test("returns a renderable icon component for every category", () => {
    for (const name of ["a.ts", "b.json", "c.md", "d.css", "e.toml", "f.sh", "g.png", "h.zip", "i.unknown"]) {
      expect(typeof fileIconFor(name).Icon).toBe("object");
    }
  });
});
