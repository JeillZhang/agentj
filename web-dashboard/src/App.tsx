import { useEffect, useState } from 'react';
import { RpcClient, ResponseRouter } from './jsonrpc';
import { WebSocketTransport } from './transport/websocket';

// G8: browser connects over WebSocket; URL configurable via VITE env.
const wsUrl = import.meta.env.VITE_AGENTJ_WS_URL ?? 'ws://127.0.0.1:4096/ws';

export default function App() {
  const [modules, setModules] = useState<{ id: string; state: string }[]>([]);
  const [status, setStatus] = useState(`connecting to kernel on ${wsUrl}…`);

  useEffect(() => {
    const router = new ResponseRouter();
    const transport = new WebSocketTransport(wsUrl, router);
    transport.onStatusChange = (s) => setStatus(`kernel ${s} on ${wsUrl}`);
    const client = new RpcClient(transport, router);
    client.setNotificationHandler((method, params) => {
      if (method === '$/progress') {
        // optionally surface progress; notifications no longer dropped (BUG-005)
        console.debug('[agentj]', method, params);
      }
    });
    client
      .request('kernel/status')
      .then((r) => {
        setModules(
          (r as { modules: { id: string; state: string }[] }).modules ?? [],
        );
        setStatus('connected');
      })
      .catch(() => setStatus(`kernel unreachable on ${wsUrl}`));

    return () => transport.close();
  }, []);

  return (
    <main>
      <h1>AgentJ Dashboard</h1>
      <p>{status}</p>
      <ul>
        {modules.map((m) => (
          <li key={m.id}>
            {m.id}: {m.state}
          </li>
        ))}
      </ul>
    </main>
  );
}
