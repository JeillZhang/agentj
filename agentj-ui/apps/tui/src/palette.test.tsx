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

describe("renderModelList", () => {
  test("model selector preselects default_model", () => {
    const out = renderModelList({
      default_model: "deepseek-chat",
      providers: [{ name: "deepseek-chat" }, { name: "opencode-go/kimi-k3" }],
    });
    expect(out).toContain("▶ deepseek-chat");
    expect(out).not.toContain("model=");
    expect(out).toContain("运行期可切换");
  });

  test("hint line exact", () => {
    const out = renderModelList({
      default_model: "deepseek-chat",
      providers: [{ name: "deepseek-chat" }],
    });
    expect(out).toContain("model 运行期可切换 · Enter 选择 · j/k 移动");
  });

  test("preselects only default", () => {
    const out = renderModelList({
      default_model: "deepseek-chat",
      providers: [{ name: "deepseek-chat" }, { name: "opencode-go/kimi-k3" }],
    });
    expect(out).toContain("▶ deepseek-chat");
    expect(out).toContain("  opencode-go/kimi-k3");
    expect(out).not.toContain("▶ opencode-go/kimi-k3");
  });

  test("no wire model param in output", () => {
    const out = renderModelList({
      default_model: "deepseek-chat",
      providers: [{ name: "deepseek-chat" }],
    });
    expect(out).not.toMatch(/model=/);
    expect(out).not.toMatch(/"model"/);
  });
});
describe("ModelSelector display", () => {
  test("renders preselected arrow and run-time switch hint", async () => {
    const setup = await testRender(
      <ModelSelector
        default_model="deepseek-chat"
        providers={[{ name: "deepseek-chat" }, { name: "opencode-go/kimi-k3" }]}
      />,
      {},
    );
    await act(async () => {
      await setup.flush();
    });
    const frame = setup.captureCharFrame();
    expect(frame).toContain("deepseek-chat");
    expect(frame).toContain("opencode-go/kimi-k3");
    expect(frame).toContain("运行期可切换");
    expect(frame).toContain("Enter");
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
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("connected"));
    });
    await act(async () => {
      await setup.flush();
    });
    expect(filterCommands("/mod", [...COMMANDS])).toEqual(["models"]);
    const out = renderModelList({
      default_model: "deepseek-chat",
      providers: [{ name: "deepseek-chat" }, { name: "opencode-go/kimi-k3" }],
    });
    expect(out).toContain("▶ deepseek-chat");
    expect(out).toContain("运行期可切换");
    act(() => setup.renderer.destroy());
  });
});
