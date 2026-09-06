import { describe, expect, it } from "vitest";
import { RpcClient } from "../client.js";
import { PROTOCOL_VERSION } from "../config.js";
import type { RpcTransport } from "../router.js";

class CaptureTransport implements RpcTransport {
  lastParams: Record<string, unknown> | null = null;
  lastMethod: string | null = null;
  async send(line: string): Promise<string> {
    const req = JSON.parse(line) as {
      id: number;
      method: string;
      params: Record<string, unknown>;
    };
    this.lastMethod = req.method;
    this.lastParams = req.params;
    return JSON.stringify({
      jsonrpc: "2.0",
      id: req.id,
      result: { protocol_version: req.params.protocol_version },
    });
  }
  close() {}
}

describe("B-001 RpcClient.initialize default", () => {
  it("defaults protocol_version to PROTOCOL_VERSION (0.12.1)", async () => {
    const transport = new CaptureTransport();
    const client = new RpcClient(transport);
    await client.initialize();
    expect(transport.lastMethod).toBe("initialize");
    expect(transport.lastParams?.protocol_version).toBe(PROTOCOL_VERSION);
    expect(PROTOCOL_VERSION).toBe("0.12.1");
  });

  it("allows explicit override still works", async () => {
    const transport = new CaptureTransport();
    const client = new RpcClient(transport);
    await client.initialize("9.9.9");
    expect(transport.lastParams?.protocol_version).toBe("9.9.9");
  });
});
