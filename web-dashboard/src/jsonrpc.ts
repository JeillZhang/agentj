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

export interface RpcTransport {
  send: (line: string) => Promise<string>;
}

export class RpcClient {
  private readonly transport: RpcTransport;

  constructor(transport: RpcTransport) {
    this.transport = transport;
  }

  async request(
    method: string,
    params: Record<string, unknown> = {},
  ): Promise<unknown> {
    const id = Date.now();
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
    let buffer = '';
    return new RpcClient({
      send: (line: string) =>
        new Promise<string>((resolve, reject) => {
          socket.write(line + '\n');
          const onData = (chunk: Buffer) => {
            buffer += chunk.toString();
            const idx = buffer.indexOf('\n');
            if (idx >= 0) {
              const response = buffer.slice(0, idx);
              buffer = buffer.slice(idx + 1);
              socket.off('data', onData);
              resolve(response);
            }
          };
          socket.on('data', onData);
          socket.on('error', reject);
        }),
    });
  }
}
