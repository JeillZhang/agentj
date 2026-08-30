import { useEffect, useState } from 'react';
import { RpcClient } from './jsonrpc';

export default function App() {
  const [modules, setModules] = useState<{ id: string; state: string }[]>([]);
  const [status, setStatus] = useState('connecting to kernel on 127.0.0.1:4096…');

  useEffect(() => {
    try {
      const client = RpcClient.connect(4096);
      client
        .request('kernel/status')
        .then((r) => {
          setModules(
            (r as { modules: { id: string; state: string }[] }).modules ?? [],
          );
          setStatus('connected');
        })
        .catch(() => setStatus('kernel unreachable on 127.0.0.1:4096'));
    } catch {
      setStatus('transport unavailable in this environment');
    }
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
