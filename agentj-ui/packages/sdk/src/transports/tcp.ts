import { Socket } from "node:net";
import type { RpcTransport } from "../router.js";

interface Pending {
  resolve: (line: string) => void;
  reject: (err: Error) => void;
}

function extractId(line: string): number {
  const parsed: unknown = JSON.parse(line);
  if (parsed && typeof parsed === "object" && "id" in parsed) {
    const idCandidate = parsed.id;
    if (typeof idCandidate === "number") return idCandidate;
  }
  throw new Error("missing id in request");
}

export class TcpTransport implements RpcTransport {
  private readonly socket: Socket;
  private buffer = "";
  private readonly pending = new Map<number, Pending>();

  private constructor(socket: Socket) {
    this.socket = socket;
    this.socket.on("data", (chunk: Buffer) => this.onData(chunk.toString()));
    this.socket.on("error", (err: Error) => this.rejectAll(err));
    this.socket.on("close", () => this.rejectAll(new Error("tcp closed")));
  }

  static async connect(host: string, port: number): Promise<TcpTransport> {
    const socket = new Socket();
    await new Promise<void>((resolve, reject) => {
      socket.once("error", reject);
      socket.connect(port, host, () => {
        socket.off("error", reject);
        resolve();
      });
    });
    return new TcpTransport(socket);
  }

  send(line: string): Promise<string> {
    const id = extractId(line);
    return new Promise<string>((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.socket.write(line + "\n", (err?: Error | null) => {
        if (err) {
          this.pending.delete(id);
          reject(err);
        }
      });
    });
  }

  close(): void {
    this.rejectAll(new Error("tcp closed"));
    this.socket.destroy();
  }

  private onData(chunk: string): void {
    this.buffer += chunk;
    let nl: number;
    while ((nl = this.buffer.indexOf("\n")) >= 0) {
      const raw = this.buffer.slice(0, nl).trim();
      this.buffer = this.buffer.slice(nl + 1);
      if (!raw) continue;
      let msg: unknown;
      try {
        msg = JSON.parse(raw);
      } catch {
        continue;
      }
      if (!msg || typeof msg !== "object" || !("id" in msg)) continue;
      const idVal = msg.id;
      if (typeof idVal !== "number") continue;
      const entry = this.pending.get(idVal);
      if (!entry) continue;
      this.pending.delete(idVal);
      entry.resolve(raw);
    }
  }

  private rejectAll(err: Error): void {
    for (const entry of this.pending.values()) entry.reject(err);
    this.pending.clear();
  }
}
