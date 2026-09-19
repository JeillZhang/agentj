// agentj-ui/apps/web/src/copyText.test.ts
import { afterEach, describe, expect, it, vi } from "vitest";
import { copyText } from "./copyText";

function setClipboard(value: unknown): void {
  Object.defineProperty(navigator, "clipboard", { value, configurable: true });
}

describe("copyText", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    delete (document as unknown as { execCommand?: unknown }).execCommand;
  });

  it("uses navigator.clipboard when available", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    setClipboard({ writeText });
    await expect(copyText("hello")).resolves.toBe(true);
    expect(writeText).toHaveBeenCalledWith("hello");
  });

  it("falls back to textarea + execCommand when clipboard rejects", async () => {
    setClipboard({ writeText: vi.fn().mockRejectedValue(new Error("denied")) });
    const exec = vi.fn().mockReturnValue(true);
    (document as unknown as { execCommand: unknown }).execCommand = exec;
    await expect(copyText("fallback")).resolves.toBe(true);
    expect(exec).toHaveBeenCalledWith("copy");
  });

  it("returns false when both paths fail without throwing", async () => {
    setClipboard(undefined);
    (document as unknown as { execCommand: unknown }).execCommand = vi.fn().mockReturnValue(false);
    await expect(copyText("nope")).resolves.toBe(false);
  });
});
