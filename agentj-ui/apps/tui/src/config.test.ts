import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join } from "node:path";
import { getConfigDir, readJsonFile, writeJsonFile } from "./config.ts";

describe("config", () => {
  let dir: string;
  let prevXdg: string | undefined;

  beforeEach(() => {
    prevXdg = process.env.XDG_CONFIG_HOME;
    dir = mkdtempSync(join(tmpdir(), "aj-config-"));
    process.env.XDG_CONFIG_HOME = dir;
  });

  afterEach(() => {
    if (prevXdg === undefined) delete process.env.XDG_CONFIG_HOME;
    else process.env.XDG_CONFIG_HOME = prevXdg;
    rmSync(dir, { recursive: true, force: true });
  });

  test("getConfigDir resolves XDG_CONFIG_HOME to <xdg>/agentj", () => {
    expect(getConfigDir()).toBe(join(dir, "agentj"));
  });

  test("getConfigDir falls back to ~/.config/agentj when XDG_CONFIG_HOME is unset or empty", () => {
    delete process.env.XDG_CONFIG_HOME;
    expect(getConfigDir()).toBe(join(homedir(), ".config", "agentj"));
    process.env.XDG_CONFIG_HOME = "";
    expect(getConfigDir()).toBe(join(homedir(), ".config", "agentj"));
  });

  test("readJsonFile returns null for a missing file", () => {
    expect(readJsonFile("missing.json")).toBeNull();
  });

  test("readJsonFile returns null for corrupt JSON and never throws", () => {
    mkdirSync(join(dir, "agentj"), { recursive: true });
    writeFileSync(join(dir, "agentj", "corrupt.json"), "{not json", "utf8");
    expect(readJsonFile("corrupt.json")).toBeNull();
  });

  test("readJsonFile returns null when the target is a directory", () => {
    mkdirSync(join(dir, "agentj", "dir.json"), { recursive: true });
    expect(readJsonFile("dir.json")).toBeNull();
  });

  test("writeJsonFile creates parent dirs and writes 2-space JSON", () => {
    writeJsonFile("a/b.json", { x: 1 });
    const p = join(dir, "agentj", "a", "b.json");
    expect(readFileSync(p, "utf8")).toBe(JSON.stringify({ x: 1 }, null, 2));
    expect(readJsonFile<{ x: number }>("a/b.json")).toEqual({ x: 1 });
  });

  test("writeJsonFile roundtrips arrays and nil", () => {
    writeJsonFile("list.json", ["a", "b"]);
    expect(readJsonFile<string[]>("list.json")).toEqual(["a", "b"]);
    writeJsonFile("nil.json", null);
    expect(readJsonFile("nil.json")).toBeNull();
  });
});
