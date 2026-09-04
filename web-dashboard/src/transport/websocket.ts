import type { ResponseRouter, RpcTransport } from '../jsonrpc';

/** Connection lifecycle callback: the App can surface connect state. */
export type TransportStatus = 'connecting' | 'open' | 'closed';

/** Browser-native WebSocket transport (G8). One WS Text message = one JSON-RPC
 *  message; feed() is invoked per message (no \n splitting needed). */
export class WebSocketTransport implements RpcTransport {
  private readonly ws: WebSocket;
  private readonly router: ResponseRouter;
  onStatusChange?: (s: TransportStatus) => void;

  constructor(
    url: string,
    router: ResponseRouter,
    wsFactory: (url: string) => WebSocket = (u) => new WebSocket(u),
  ) {
    this.router = router;
    this.ws = wsFactory(url);
    this.ws.onopen = () => this.onStatusChange?.('open');
    this.ws.onmessage = (event) => {
      const data = typeof event.data === 'string' ? event.data : '';
      if (data) this.router.feed(data.endsWith('\n') ? data : data + '\n');
    };
    this.ws.onerror = () => this.router.rejectAll(new Error('websocket error'));
    this.ws.onclose = () => {
      this.router.rejectAll(new Error('websocket closed'));
      this.onStatusChange?.('closed');
    };
  }

  send(line: string): Promise<string> {
    return new Promise((resolve, reject) => {
      this.router.register(this.router.requestId(line), { resolve, reject });
      this.ws.send(line);
    });
  }

  close(): void {
    this.ws.close();
  }
}
