import net from 'node:net';

export interface RpcRequest {
  jsonrpc: '2.0';
  id: number;
  method: string;
  params?: Record<string, unknown>;
}

export interface RpcResponse {
  jsonrpc: '2.0';
  id: number | null;
  result?: unknown;
  error?: { code: number; message: string };
}

export interface InitializeResult {
  protocol_version: string;
  capabilities?: { multi_client?: boolean };
  server_info?: { name: string; version: string };
  connection_id?: string;
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
  private buffer = '';
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
    while ((nl = this.buffer.indexOf('\n')) >= 0) {
      const raw = this.buffer.slice(0, nl).trim();
      this.buffer = this.buffer.slice(nl + 1);
      if (!raw) continue;
      let msg: { id?: number | null; method?: string; params?: unknown; result?: unknown; error?: unknown };
      try {
        msg = JSON.parse(raw) as { id?: number | null; method?: string; params?: unknown; result?: unknown; error?: unknown };
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
}

export class RpcClient {
  private static nextId = 1;
  private readonly transport: RpcTransport;
  private readonly router: ResponseRouter;

  constructor(transport: RpcTransport, router?: ResponseRouter) {
    this.transport = transport;
    this.router = router ?? new ResponseRouter();
  }

  setNotificationHandler(handler: NotificationHandler): void {
    this.router.setNotificationHandler(handler);
  }

  async request(
    method: string,
    params: Record<string, unknown> = {},
  ): Promise<unknown> {
    const id = RpcClient.nextId++;
    const line = JSON.stringify({ jsonrpc: '2.0', id, method, params });
    const raw = await this.transport.send(line);
    const response = JSON.parse(raw) as RpcResponse;
    if (response.error) {
      throw new Error(`${response.error.code}: ${response.error.message}`);
    }
    return response.result;
  }

  /// Reads connection_id + capabilities.multi_client from the initialize
  /// result (决策 #6, snake_case wire fields).
  async initialize(protocol_version = '0.4.0'): Promise<InitializeResult> {
    return (await this.request('initialize', {
      protocol_version,
    })) as InitializeResult;
  }

  static connect(port: number, host = '127.0.0.1'): RpcClient {
    const socket = net.createConnection({ port, host });
    const router = new ResponseRouter();
    socket.on('data', (chunk: Buffer) => router.feed(chunk.toString()));
    socket.on('error', (err: Error) => router.rejectAll(err));
    return new RpcClient({
      send: (line: string) =>
        new Promise<string>((resolve, reject) => {
          router.register(router.requestId(line), { resolve, reject });
          socket.write(line + '\n');
        }),
    });
  }
}
