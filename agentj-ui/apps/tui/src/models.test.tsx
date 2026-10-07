import { describe, expect, it, mock } from "bun:test";
import { testRender } from "@opentui/react/test-utils";
import { act } from "react";
import { MODEL_CONFIG, ModelSelector, providersFromStatus, renderModelList } from "./models";

describe("providersFromStatus", () => {
  it("tags entries with their provider id", () => {
    const data = {
      providers: [{ id: "opencode-go", models: ["opencode-go/a", "opencode-go/b"] }],
    };
    expect(providersFromStatus(data)).toEqual([
      { name: "opencode-go/a", provider: "opencode-go" },
      { name: "opencode-go/b", provider: "opencode-go" },
    ]);
  });

  it("tags each entry with its own provider id", () => {
    const data = {
      providers: [
        { id: "zz-provider", models: ["zz/a"] },
        { id: "aa-provider", models: ["aa/b"] },
      ],
    };
    expect(providersFromStatus(data)).toEqual([
      { name: "zz/a", provider: "zz-provider" },
      { name: "aa/b", provider: "aa-provider" },
    ]);
  });

  it("falls back to the static provider list without provider tags", () => {
    expect(providersFromStatus({ providers: [{ id: "opencode-go", models: [] }] })).toEqual(
      MODEL_CONFIG.providers.map(({ name }) => ({ name })),
    );
  });

  it("falls back when providers is missing", () => {
    expect(providersFromStatus({})).toEqual(MODEL_CONFIG.providers.map(({ name }) => ({ name })));
  });
});

describe("renderModelList", () => {
  it("marks the default model when nothing is selected", () => {
    const out = renderModelList({
      default_model: "opencode-go/a",
      providers: [{ name: "opencode-go/a" }, { name: "opencode-go/b" }],
    });
    const lines = out.split("\n");
    expect(lines[0]).toBe("▶ opencode-go/a");
    expect(lines[1]).toBe("  opencode-go/b");
  });

  it("marks the selected model over the default", () => {
    const out = renderModelList({
      default_model: "opencode-go/a",
      providers: [{ name: "opencode-go/a" }, { name: "opencode-go/b" }],
      selected: { name: "opencode-go/b" },
    });
    const lines = out.split("\n");
    expect(lines[0]).toBe("  opencode-go/a");
    expect(lines[1]).toBe("▶ opencode-go/b");
  });

  it("hint text no longer mentions restarting the kernel", () => {
    const out = renderModelList({ default_model: "a", providers: [{ name: "a" }] });
    expect(out).not.toContain("重启内核生效");
    expect(out).toContain("Enter");
  });
});

describe("ModelSelector", () => {
  it("selects the highlighted entry on Enter", async () => {
    const onSelect = mock(() => {});
    const setup = await testRender(
      <ModelSelector
        default_model="a"
        providers={[{ name: "a" }, { name: "b" }]}
        onSelect={onSelect}
      />,
      {},
    );
    await act(async () => {
      setup.mockInput.pressKey("j");
      await setup.flush();
    });
    await act(async () => {
      setup.mockInput.pressEnter();
      await setup.flush();
    });
    expect(onSelect).toHaveBeenCalledWith({ name: "b" });
    act(() => setup.renderer.destroy());
  });

  it("starts the highlight on the selected entry and moves back with k", async () => {
    const onSelect = mock(() => {});
    const setup = await testRender(
      <ModelSelector
        default_model="a"
        providers={[{ name: "a" }, { name: "b" }, { name: "c" }]}
        selected={{ name: "c" }}
        onSelect={onSelect}
      />,
      {},
    );
    await act(async () => {
      setup.mockInput.pressKey("k");
      await setup.flush();
    });
    await act(async () => {
      setup.mockInput.pressEnter();
      await setup.flush();
    });
    expect(onSelect).toHaveBeenCalledWith({ name: "b" });
    act(() => setup.renderer.destroy());
  });
});
