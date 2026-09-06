import { METHODS, ResponseRouter, RpcClient } from "@agentj/sdk";
import type { RpcTransport, TransportStatus } from "@agentj/sdk";

export type TimerHandle = number;

export interface TransportHandle extends RpcTransport {
  onStatusChange?: (s: TransportStatus) => void;
  close(): void;
  lastError?: string;
}

export type CreateTransport = (url: string, router: ResponseRouter) => TransportHandle;

export function getBackoffDelay(attempt: number, baseMs = 1000): number {
  return Math.min(baseMs * Math.pow(2, attempt), 30000);
}

function isAuthError(msg: string): boolean {
  const low = msg.toLowerCase();
  return (
    msg.includes("403") ||
    msg.includes("-32806") ||
    low.includes("notauthorized") ||
    low.includes("auth") ||
    low.includes("forbidden")
  );
}
export function restoreTerminal(): void {
  try {
    const stdin = process.stdin as unknown as NodeJS.ReadStream;
    if (process.stdin.isTTY && typeof stdin.setRawMode === "function") stdin.setRawMode(false);
  } catch {}
  try {
    process.stdout.write("\x1b[?25h");
  } catch {}
  try {
    process.stdout.write("\x1b[?1049l");
  } catch {}
  try {
    const g = globalThis as unknown as Record<string, unknown>;
    if ("__opentui_renderer" in g) {
      const r = g["__opentui_renderer"] as { destroy?: () => void } | undefined;
      r?.destroy?.();
    }
  } catch {}
}

export function setupGracefulShutdown(restore: () => void = restoreTerminal): () => void {
  const handler = () => {
    try {
      restore();
    } catch {}
    process.exit(0);
  };
  process.on("SIGINT", handler);
  process.on("SIGTERM", handler);
  const cleanup = () => {
    process.off("SIGINT", handler);
    process.off("SIGTERM", handler);
  };
  return cleanup;
}

export class ConnectionManager {
  activeSessionId: string | null = null;
  private attempt = 0;
  private timer: TimerHandle | null = null;
  private transport: TransportHandle | null = null;
  private router: ResponseRouter | null = null;
  private client: RpcClient | null = null;
  status: TransportStatus = "connecting";
  onStatusChange?: (s: TransportStatus) => void;

  constructor(
    private url: string,
    private createTransport: CreateTransport,
  ) {}

  setActiveSessionId(id: string | null): void {
    this.activeSessionId = id;
  }

  getActiveSessionId(): string | null {
    return this.activeSessionId;
  }

  getClient(): RpcClient | null {
    return this.client;
  }

  connect(): void {
    if (this.transport) {
      try {
        this.transport.close();
      } catch {}
    }
    const router = new ResponseRouter();
    this.router = router;
    let transport: TransportHandle;
    try {
      transport = this.createTransport(this.url, router);
    } catch {
      this.handleStatus("closed");
      return;
    }
    this.transport = transport;
    try {
      const t = transport as unknown as { send: (l: string) => Promise<string> };
      if (typeof t.send === "function") this.client = new RpcClient(t, router);
    } catch {}
    this.attachStatus(transport);
    if (transport.lastError) this.handleStatus("closed");
  }

  private attachStatus(transport: TransportHandle): void {
    const prev = transport.onStatusChange;
    transport.onStatusChange = (s: TransportStatus) => {
      this.handleStatus(s);
      prev?.(s);
    };
  }

  private handleStatus(s: TransportStatus): void {
    this.status = s;
    if (s === "open") {
      this.attempt = 0;
      this.onStatusChange?.(s);
      void this.resubscribe();
      return;
    }
    if (s === "closed") {
      this.onStatusChange?.(s);
      const err = this.transport?.lastError ?? "";
      if (err && isAuthError(err)) {
        return;
      }
      this.scheduleReconnect();
    } else {
      this.onStatusChange?.(s);
    }
  }

  private scheduleReconnect(): void {
    clearTimeout(this.timer as unknown as number);
    const delay = getBackoffDelay(this.attempt, 1000);
    this.attempt += 1;
    this.timer = setTimeout(() => this.connect(), delay) as unknown as TimerHandle;
  }

  private async resubscribe(): Promise<void> {
    if (!this.activeSessionId) return;
    if (!this.client) return;
    try {
      await this.client.request(METHODS.sessionSubscribe, {
        session_id: this.activeSessionId,
      });
    } catch {}
  }

  destroy(): void {
    clearTimeout(this.timer as unknown as number);
    this.timer = null;
    try {
      this.transport?.close();
    } catch {}
    this.transport = null;
    this.client = null;
  }
}
