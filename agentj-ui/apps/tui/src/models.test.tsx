import { describe, expect, it } from "bun:test";
import { MODEL_CONFIG, providersFromStatus, renderModelList } from "./models";

describe("providersFromStatus", () => {
  it("flattens models from served providers", () => {
    const data = {
      providers: [{ id: "opencode-go", models: ["opencode-go/a", "opencode-go/b"] }],
    };
    expect(providersFromStatus(data)).toEqual(["opencode-go/a", "opencode-go/b"]);
  });

  it("falls back when no provider reports models", () => {
    expect(providersFromStatus({ providers: [{ id: "opencode-go", models: [] }] })).toEqual(
      MODEL_CONFIG.providers,
    );
  });

  it("falls back when providers is missing", () => {
    expect(providersFromStatus({})).toEqual(MODEL_CONFIG.providers);
  });
});

describe("renderModelList", () => {
  it("marks the default model with the active marker", () => {
    const out = renderModelList({
      default_model: "opencode-go/a",
      providers: ["opencode-go/a", "opencode-go/b"],
    });
    const lines = out.split("\n");
    expect(lines[0]).toBe("▶ opencode-go/a");
    expect(lines[1]).toBe("  opencode-go/b");
  });
});
