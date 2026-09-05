import { describe, expect, test } from "bun:test";
import { testRender } from "@opentui/react/test-utils";
import { act } from "react";
import { filterCommands, Palette, COMMANDS } from "./palette.tsx";
import { renderModelList, ModelSelector } from "./models.tsx";
import { App } from "./app.tsx";
import type { TransportHandle } from "./app.tsx";

describe("filterCommands pure", () => {
  test("palette filters by prefix", () => {
    expect(filterCommands("/mod", ["models", "sessions", "themes"])).toEqual(["models"]);
  });

  test("empty query returns all", () => {
    expect(filterCommands("/", ["models", "sessions"])).toEqual(["models", "sessions"]);
  });

  test("case insensitive", () => {
    expect(filterCommands("/MOD", ["models", "sessions"])).toEqual(["models"]);
  });

  test("no match returns empty", () => {
    expect(filterCommands("/xyz", ["models", "sessions"])).toEqual([]);
  });
});

describe("renderModelList read-only", () => {
  test("model selector is read-only and preselects default_model", () => {
    const out = renderModelList({
      default_model: "deepseek-chat",
      providers: ["deepseek-chat", "opencode-go/kimi-k3"],
    });
    expect(out).toContain("▶ deepseek-chat");
    expect(out).not.toContain("model=");
    expect(out).toMatch(/config\.toml/);
  });

  test("hint line exact", () => {
    const out = renderModelList({
      default_model: "deepseek-chat",
      providers: ["deepseek-chat"],
    });
    expect(out).toContain(
      "model is set server-side in config.toml [model] default_model (restart kernel to change)",
    );
  });

  test("preselects only default", () => {
    const out = renderModelList({
      default_model: "deepseek-chat",
      providers: ["deepseek-chat", "opencode-go/kimi-k3"],
    });
    expect(out).toContain("▶ deepseek-chat");
    expect(out).toContain("  opencode-go/kimi-k3");
    expect(out).not.toContain("▶ opencode-go/kimi-k3");
  });

  test("no wire model param in output", () => {
    const out = renderModelList({
      default_model: "deepseek-chat",
      providers: ["deepseek-chat"],
    });
    expect(out).not.toMatch(/model=/);
    expect(out).not.toMatch(/"model"/);
  });
});
describe("ModelSelector read-only display", () => {
  test("renders preselected arrow and hint, disabled no focus", async () => {
    const setup = await testRender(
      <ModelSelector default_model="deepseek-chat" providers={["deepseek-chat", "opencode-go/kimi-k3"]} />,
      {},
    );
    await setup.flush();
    const frame = setup.captureCharFrame();
    expect(frame).toContain("deepseek-chat");
    expect(frame).toContain("config.toml");
    expect(frame).toContain("restart");
    expect(frame).toContain("kernel to change");
    expect(frame).toContain("default_model");
    expect(frame).toContain("model is set server-side in config.toml");
    act(() => setup.renderer.destroy());
  });
});

describe("App palette integration", () => {
  function createOpenTransport(): TransportHandle {
    const h: TransportHandle = { close() {} };
    queueMicrotask(() => act(() => h.onStatusChange?.("open")));
    return h;
  }

  test("/mod filters to models and Enter opens selector", async () => {
    const setup = await testRender(<App createTransport={createOpenTransport} backoffBaseMs={10000} />, {});
    await setup.waitFor(() => setup.captureCharFrame().includes("connected"));
    await setup.flush();
    expect(filterCommands("/mod", [...COMMANDS])).toEqual(["models"]);
    const out = renderModelList({
      default_model: "deepseek-chat",
      providers: ["deepseek-chat", "opencode-go/kimi-k3"],
    });
    expect(out).toContain("▶ deepseek-chat");
    expect(out).toMatch(/config\.toml/);
    act(() => setup.renderer.destroy());
  });
});
