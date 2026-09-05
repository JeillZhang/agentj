import type { ResponseRouter, RpcTransport } from '../jsonrpc';

/** Connection lifecycle callback: the App can surface connect state. */
export type TransportStatus = 'connecting' | 'open' | 'closed';

interface QueuedSend {
  line: string;
  reject: (err: Error) => void;
}

/** Browser-native WebSocket transport (G8). One WS Text message = one JSON-RPC
 *  message; ResponseRouter is NDJSON-delimited, append \n so single Text frame flushes. */
export class WebSocketTransport implements RpcTransport {
  private readonly ws: WebSocket;
  private readonly router: ResponseRouter;
  private queued: QueuedSend[] = [];
  onStatusChange?: (s: TransportStatus) => void;

  constructor(
    url: string,
    router: ResponseRouter,
    wsFactory: (url: string) => WebSocket = (u) => new WebSocket(u),
  ) {
    this.router = router;
    this.ws = wsFactory(url);
    this.onStatusChange?.('connecting');
    this.ws.onopen = () => {
      this.onStatusChange?.('open');
      this.flushQueued();
    };
    this.ws.onmessage = (event) => {
      const data = typeof event.data === 'string' ? event.data : '';
      // ResponseRouter is NDJSON-delimited, append \n so single Text frame flushes
      if (data) this.router.feed(data.endsWith('\n') ? data : data + '\n');
    };
    this.ws.onerror = () => {
      this.router.rejectAll(new Error('websocket error'));
      this.onStatusChange?.('closed');
    };
    this.ws.onclose = () => {
      this.router.rejectAll(new Error('websocket closed'));
      this.onStatusChange?.('closed');
    };
  }

  send(line: string): Promise<string> {
    return new Promise((resolve, reject) => {
      this.router.register(this.router.requestId(line), { resolve, reject });
      if (this.ws.readyState === WebSocket.OPEN) {
        this.ws.send(line);
      } else if (this.ws.readyState === WebSocket.CONNECTING) {
        // App sends kernel/status in the same tick as construction; queue the
        // line and flush on open (BUG-002: raw send throws InvalidStateError).
        this.queued.push({ line, reject });
      } else {
        reject(new Error('websocket not open'));
      }
    });
  }

  private flushQueued(): void {
    const queued = this.queued;
    this.queued = [];
    for (const entry of queued) {
      try {
        this.ws.send(entry.line);
      } catch (err) {
        entry.reject(err instanceof Error ? err : new Error(String(err)));
      }
    }
  }

  close(): void {
    this.ws.close();
  }
}
