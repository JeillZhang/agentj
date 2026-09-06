import { createServer } from "node:net";
import type { Socket } from "node:net";
import { describe, expect, it } from "vitest";
import { TcpTransport } from "../transports/tcp.js";

describe("TcpTransport", () => {
  it("tcp transport resolves by id over NDJSON", async () => {
    const server = createServer((s: Socket) =>
      s.on("data", (b: Buffer) => {
        const req = JSON.parse(b.toString().split("\n")[0]);
        s.write(
          JSON.stringify({ jsonrpc: "2.0", id: req.id, result: { ok: true } }) +
            "\n",
        );
      }),
    );
    await new Promise<void>((r) => server.listen(0, r));
    const port = (server.address() as { port: number }).port;
    const t = await TcpTransport.connect("127.0.0.1", port);
    const res = await t.send(
      JSON.stringify({ jsonrpc: "2.0", id: 7, method: "kernel/status" }),
    );
    expect(JSON.parse(res).result).toEqual({ ok: true });
    t.close();
    server.close();
  });

  it("close rejects pending requests", async () => {
    const server = createServer(() => {});
    await new Promise<void>((r) => server.listen(0, r));
    const port = (server.address() as { port: number }).port;
    const t = await TcpTransport.connect("127.0.0.1", port);
    const pending = t.send(
      JSON.stringify({ jsonrpc: "2.0", id: 8, method: "kernel/status" }),
    );
    t.close();
    await expect(pending).rejects.toThrow();
    server.close();
  });

});
