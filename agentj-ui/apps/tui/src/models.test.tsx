import { describe, expect, it, mock } from "bun:test";
import { testRender } from "@opentui/react/test-utils";
import type { TestRendererSetup } from "@opentui/core/testing";
import { act } from "react";
import type { RpcClient } from "@agentj/sdk";
import {
  MODEL_CONFIG,
  ModelSelector,
  providersFromStatus,
  renderModelList,
  useModelConfig,
} from "./models";
import type { ModelListInput } from "./models";

// D1: two providers may serve the same model id; the row is identified by
// name + provider, exactly like the Web picker's modelKey.
const DUP_PROVIDERS = [
  { name: "dup", provider: "aaa" },
  { name: "dup", provider: "zzz" },
];

type ConfigControl = { current: ModelListInput | null };

function ModelConfigProbe({ client, control }: {
  client: RpcClient | null;
  control: ConfigControl;
}) {
  const cfg = useModelConfig(client);
  control.current = cfg;
  return <text>{cfg.providers.map((e) => `${e.provider ?? "-"}:${e.name}`).join(",")}</text>;
}

async function renderConfigProbe(
  data: Record<string, unknown>,
): Promise<{ cfg: () => ModelListInput; setup: TestRendererSetup }> {
  let settle: (value: unknown) => void = () => {};
  const pending = new Promise<unknown>((resolve) => {
    settle = resolve;
  });
  const client = { request: () => pending } as unknown as RpcClient;
  const control: ConfigControl = { current: null };
  const setup = await testRender(<ModelConfigProbe client={client} control={control} />, {});
  await act(async () => {
    settle(data);
    await setup.flush();
  });
  return { cfg: () => control.current!, setup };
}

function markerCount(frame: string): number {
  return (frame.match(/▶/g) ?? []).length;
}

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

  it("marks only the picked row when two providers serve the same model id", () => {
    const out = renderModelList({
      default_model: "dup",
      providers: DUP_PROVIDERS,
      selected: { name: "dup", provider: "zzz" },
    });
    expect(out.match(/▶/g)).toHaveLength(1);
    expect(out.split("\n").slice(0, 2)).toEqual(["  dup", "▶ dup"]);
  });

  it("marks no row when the default name is served by several providers", () => {
    const out = renderModelList({ default_model: "dup", providers: DUP_PROVIDERS });
    expect(out).not.toContain("▶");
    expect(out.split("\n").slice(0, 2)).toEqual(["  dup", "  dup"]);
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

  it("submits the selected provider entry when two providers serve the same model id", async () => {
    const onSelect = mock(() => {});
    const setup = await testRender(
      <ModelSelector
        default_model="dup"
        providers={DUP_PROVIDERS}
        selected={{ name: "dup", provider: "zzz" }}
        onSelect={onSelect}
      />,
      {},
    );
    await act(async () => {
      await setup.flush();
    });
    expect(markerCount(setup.captureCharFrame())).toBe(1);
    await act(async () => {
      setup.mockInput.pressEnter();
      await setup.flush();
    });
    expect(onSelect).toHaveBeenCalledWith({ name: "dup", provider: "zzz" });
    act(() => setup.renderer.destroy());
  });

  it("marks no row and keeps the cursor on the first entry for an ambiguous default", async () => {
    const onSelect = mock(() => {});
    const setup = await testRender(
      <ModelSelector default_model="dup" providers={DUP_PROVIDERS} onSelect={onSelect} />,
      {},
    );
    await act(async () => {
      await setup.flush();
    });
    expect(markerCount(setup.captureCharFrame())).toBe(0);
    await act(async () => {
      setup.mockInput.pressEnter();
      await setup.flush();
    });
    expect(onSelect).toHaveBeenCalledWith({ name: "dup", provider: "aaa" });
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

describe("useModelConfig", () => {
  it("accepts kernel providers when default_model is absent", async () => {
    const { cfg, setup } = await renderConfigProbe({
      providers: [{ id: "opencode-go", models: ["opencode-go/kimi-k3"] }],
    });
    expect(cfg().providers).toEqual([{ name: "opencode-go/kimi-k3", provider: "opencode-go" }]);
    expect(cfg().default_model).toBe("");
    act(() => setup.renderer.destroy());
  });

  it("accepts kernel providers when default_model is empty", async () => {
    const { cfg, setup } = await renderConfigProbe({
      default_model: "",
      providers: [
        { id: "deepseek", models: ["deepseek-chat"] },
        { id: "opencode-go", models: ["opencode-go/kimi-k3"] },
      ],
    });
    expect(cfg().providers).toEqual([
      { name: "deepseek-chat", provider: "deepseek" },
      { name: "opencode-go/kimi-k3", provider: "opencode-go" },
    ]);
    expect(cfg().default_model).toBe("");
    act(() => setup.renderer.destroy());
  });

  it("keeps the static list when the kernel reports no models at all", async () => {
    const { cfg, setup } = await renderConfigProbe({ default_model: "deepseek-chat" });
    expect(cfg().providers).toEqual(MODEL_CONFIG.providers.map(({ name }) => ({ name })));
    expect(cfg().default_model).toBe("deepseek-chat");
    act(() => setup.renderer.destroy());
  });
});
