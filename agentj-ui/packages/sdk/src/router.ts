export interface RpcRequest {
  jsonrpc: "2.0";
  id: number;
  method: string;
  params?: Record<string, unknown>;
}

export type NotificationHandler = (method: string, params: unknown) => void;

export interface RpcTransport {
  send: (line: string) => Promise<string>;
  /** Optional release hook (e.g. WebSocket.close). */
  close?: () => void;
}

interface PendingEntry {
  resolve: (line: string) => void;
  reject: (err: Error) => void;
}

/// Buffers incoming NDJSON lines and resolves each request promise with the
/// line whose `id` matches the request id. JSON-RPC notifications (no id,
/// e.g. `$/progress`) are forked to the notification handler (BUG-005 fix).
export class ResponseRouter {
  private buffer = "";
  private readonly pending = new Map<number, PendingEntry>();
  private notificationHandler?: NotificationHandler;

  setNotificationHandler(handler: NotificationHandler): void {
    this.notificationHandler = handler;
  }

  requestId(line: string): number {
    return (JSON.parse(line) as RpcRequest).id;
  }

  register(id: number, entry: PendingEntry): void {
    this.pending.set(id, entry);
  }

  feed(chunk: string): void {
    this.buffer += chunk;
    let nl: number;
    while ((nl = this.buffer.indexOf("\n")) >= 0) {
      const raw = this.buffer.slice(0, nl).trim();
      this.buffer = this.buffer.slice(nl + 1);
      if (!raw) continue;
      let msg: {
        id?: number | null;
        method?: string;
        params?: unknown;
        result?: unknown;
        error?: unknown;
      };
      try {
        msg = JSON.parse(raw) as {
          id?: number | null;
          method?: string;
          params?: unknown;
          result?: unknown;
          error?: unknown;
        };
      } catch {
        continue;
      }
      if (msg.id === undefined || msg.id === null) {
        // Notification (BUG-005 fix): dispatch, never drop.
        if (msg.method && this.notificationHandler) {
          this.notificationHandler(msg.method, msg.params);
        }
        continue;
      }
      const entry = this.pending.get(msg.id);
      if (!entry) continue;
      this.pending.delete(msg.id);
      entry.resolve(raw);
    }
  }

  rejectAll(err: Error): void {
    for (const entry of this.pending.values()) {
      entry.reject(err);
    }
    this.pending.clear();
  }

  pendingCount(): number {
    return this.pending.size;
  }
}
