import type { NotificationHandler, RpcTransport } from "./router.js";
import { ResponseRouter } from "./router.js";
import { PROTOCOL_VERSION } from "./config.js";

export interface RpcResponse {
  jsonrpc: "2.0";
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

  removeNotificationHandler(handler: NotificationHandler): void {
    if (typeof (this.router as unknown as { removeNotificationHandler?: (h: NotificationHandler) => void }).removeNotificationHandler === "function") {
      (this.router as unknown as { removeNotificationHandler: (h: NotificationHandler) => void }).removeNotificationHandler(handler);
    }
  }

  async request(
    method: string,
    params: Record<string, unknown> = {},
  ): Promise<unknown> {
    const id = RpcClient.nextId++;
    const line = JSON.stringify({ jsonrpc: "2.0", id, method, params });
    const raw = await this.transport.send(line);
    const response = JSON.parse(raw) as RpcResponse;
    if (response.error) {
      throw new Error(`${response.error.code}: ${response.error.message}`);
    }
    return response.result;
  }

  /// Reads connection_id + capabilities.multi_client from the initialize
  /// result (决策 #6, snake_case wire fields).
  async initialize(protocol_version = PROTOCOL_VERSION): Promise<InitializeResult> {
    return (await this.request("initialize", {
      protocol_version,
    })) as InitializeResult;
  }
}
