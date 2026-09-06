import { describe, expect, test } from "bun:test";
import { startMockKernel, driveRun } from "./mock-kernel.ts";

describe("mock kernel 0.12 standard frames", () => {
  test("drives a run with text deltas then a usage-kind done frame", async () => {
    const k = await startMockKernel();
    const frames: Array<Record<string, unknown>> = [];
    const events = await driveRun(k.url, "hello", {
      approve: "allow",
      onFrame: (params) => frames.push(params),
    });
    expect(events).toEqual(["run", "progress", "permission", "respond", "done"]);
    const kinds = frames.map((f) => (f.value as { kind?: string })?.kind);
    expect(kinds).toEqual(["text", "text", "usage"]);
    const first = frames[0] as {
      session_id?: string;
      token?: number;
      value?: { kind: string; delta: string };
    };
    expect(first.session_id).toBe("sess-1");
    expect(first.token).toBe(1);
    expect(first.value?.kind).toBe("text");
    expect(first.value?.delta).toBe("hello progress");
    expect((frames[1] as { token?: number }).token).toBe(2);
    const last = frames[2] as { value: { kind: string; usage: { prompt_tokens: number; completion_tokens: number } } };
    expect(last.value.kind).toBe("usage");
    expect(last.value.usage).toEqual({ prompt_tokens: 10, completion_tokens: 5 });
    await k.close();
  }, 15000);

  test("initialize handshake advertises protocol 0.12.0", async () => {
    const k = await startMockKernel();
    const { WebSocket } = await import("ws");
    const ws = new WebSocket(k.url);
    await new Promise<void>((resolve, reject) => {
      ws.on("open", () => resolve());
      ws.on("error", reject);
    });
    const version = await new Promise<string>((resolve) => {
      ws.on("message", (data: Buffer) => {
        const m = JSON.parse(data.toString()) as {
          result?: { protocol_version?: string };
        };
        if (m.result?.protocol_version) resolve(m.result.protocol_version);
      });
      ws.send(
        JSON.stringify({ jsonrpc: "2.0", id: 1, method: "initialize", params: { protocol_version: "0.12.0" } }),
      );
    });
    expect(version).toBe("0.12.0");
    ws.close();
    await k.close();
  }, 5000);
});
