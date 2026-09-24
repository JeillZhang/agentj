import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { getConfigDir } from "./config.ts";
import { loadFileTreePrefs, saveFileTreePrefs } from "./filetreePrefs.ts";

describe("filetreePrefs", () => {
  let dir: string;
  let prevXdg: string | undefined;

  beforeEach(() => {
    prevXdg = process.env.XDG_CONFIG_HOME;
    dir = mkdtempSync(join(tmpdir(), "aj-filetree-"));
    process.env.XDG_CONFIG_HOME = dir;
  });

  afterEach(() => {
    if (prevXdg === undefined) delete process.env.XDG_CONFIG_HOME;
    else process.env.XDG_CONFIG_HOME = prevXdg;
    rmSync(dir, { recursive: true, force: true });
  });

  test("loadFileTreePrefs returns an empty expanded map for a missing file", () => {
    expect(loadFileTreePrefs()).toEqual({ expanded: {} });
  });

  test("loadFileTreePrefs returns an empty expanded map for corrupt JSON and never throws", () => {
    mkdirSync(join(dir, "agentj"), { recursive: true });
    writeFileSync(join(getConfigDir(), "filetree.json"), "{not json", "utf8");
    expect(loadFileTreePrefs()).toEqual({ expanded: {} });
  });

  test("loadFileTreePrefs ignores malformed shapes without throwing", () => {
    mkdirSync(join(dir, "agentj"), { recursive: true });
    const p = join(getConfigDir(), "filetree.json");
    writeFileSync(p, JSON.stringify({ expanded: { p1: "nope" } }), "utf8");
    expect(loadFileTreePrefs()).toEqual({ expanded: {} });
    writeFileSync(p, JSON.stringify({ expanded: { p1: ["src", 5, null] } }), "utf8");
    expect(loadFileTreePrefs()).toEqual({ expanded: { p1: ["src"] } });
  });

  test("saveFileTreePrefs roundtrips expanded paths preserving order", () => {
    saveFileTreePrefs({ expanded: { p1: ["src", "src/components", "docs"] } }, ["p1"]);
    expect(loadFileTreePrefs()).toEqual({ expanded: { p1: ["src", "src/components", "docs"] } });
  });

  test("caps entries per key and prunes stale project keys", () => {
    const many = Array.from({ length: 120 }, (_, i) => `d${i}`);
    saveFileTreePrefs({ expanded: { p1: many, p9: ["x"] } }, ["p1"]);
    const loaded = loadFileTreePrefs();
    expect(loaded.expanded.p1).toHaveLength(100);
    expect(loaded.expanded.p1[0]).toBe("d0");
    expect(loaded.expanded.p1[99]).toBe("d99");
    expect(loaded.expanded.p9).toBeUndefined();
  });

  test("always keeps the __default__ key and drops keys not in the live list", () => {
    saveFileTreePrefs({ expanded: { __default__: ["src"], p9: ["y"] } }, []);
    expect(loadFileTreePrefs()).toEqual({ expanded: { __default__: ["src"] } });
  });
});
