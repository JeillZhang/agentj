import { useCallback, useEffect, useRef, useState } from "react";
import {
  METHODS,
  PROTOCOL_VERSION,
  ResponseRouter,
  RpcClient,
  WebSocketTransport,
  resolveWsUrl,
} from "@agentj/sdk";
import ApprovalDialog from "./ApprovalDialog";
import type { ApprovalRequest } from "./ApprovalDialog";
import ConnectionBar from "./ConnectionBar";
import RunPanel from "./RunPanel";
import SessionPanel from "./SessionPanel";

const rawWsEnv = import.meta.env.VITE_AGENTJ_WS_URL;
const envWsUrl = typeof rawWsEnv === "string" ? rawWsEnv : undefined;

function useKernel(wsUrl: string) {
  const [modules, setModules] = useState<{ id: string; state: string }[]>([]);
  const [status, setStatus] = useState(`connecting to kernel on ${wsUrl}…`);
  const [banner, setBanner] = useState<string | null>(null);
  const [unreachable, setUnreachable] = useState(false);
  useEffect(() => {
    let cancelled = false;
    setUnreachable(false);
    setBanner(null);
    const router = new ResponseRouter();
    const transport = new WebSocketTransport(wsUrl, router);
    transport.onStatusChange = (s) => {
      if (!cancelled) setStatus(`kernel ${s} on ${wsUrl}`);
    };
    const client = new RpcClient(transport, router);
    client.setNotificationHandler((method, params) => {
      if (method === METHODS.progress) console.debug("[agentj]", method, params);
    });
    client
      .initialize(PROTOCOL_VERSION)
      .then((res) => {
        if (cancelled) return;
        if (res.protocol_version !== PROTOCOL_VERSION) {
          setBanner(`expected ${PROTOCOL_VERSION}, got ${res.protocol_version}`);
        }
      })
      .catch(() => {})
      .finally(() => {
        if (cancelled) return;
        client
          .request(METHODS.kernelStatus)
          .then((r) => {
            const data = r as { modules?: { id: string; state: string }[] };
            if (cancelled) return;
            setModules(data.modules ?? []);
            setStatus("connected");
          })
          .catch(() => {
            if (cancelled) return;
            setUnreachable(true);
            setStatus(`kernel unreachable on ${wsUrl}`);
          });
      });
    return () => {
      cancelled = true;
      transport.close();
    };
  }, [wsUrl]);
  return { modules, status, banner, unreachable };
}

function useApproval(wsUrl: string, activeSessionId: string | null) {
  const [pending, setPending] = useState<ApprovalRequest | null>(null);
  const [approvalError, setApprovalError] = useState<string | null>(null);
  const clientRef = useRef<RpcClient | null>(null);
  const activeRef = useRef(activeSessionId);
  useEffect(() => { activeRef.current = activeSessionId; }, [activeSessionId]);
  useEffect(() => {
    if (pending?.session_id && pending.session_id !== activeSessionId) {
      setPending(null);
      setApprovalError(null);
    }
  }, [activeSessionId, pending]);
  useEffect(() => {
    setPending(null);
    setApprovalError(null);
    const router = new ResponseRouter();
    const transport = new WebSocketTransport(wsUrl, router);
    const client = new RpcClient(transport, router);
    clientRef.current = client;
    client.setNotificationHandler((method, params) => {
      if (method !== METHODS.permissionRequest) return;
      const p = params as { request_id: string; tool: string; args: unknown; session_id?: string | null; reason?: string };
      const active = activeRef.current;
      if (!active) return;
      const sid = p.session_id ?? null;
      if (sid && sid !== active) return;
      setPending({ request_id: p.request_id, tool: p.tool, args: p.args, session_id: sid ?? active, reason: p.reason });
      setApprovalError(null);
    });
    return () => { clientRef.current = null; transport.close(); };
  }, [wsUrl]);
  const respond = useCallback(async (decision: string, allowAlways: boolean) => {
    const req = pending;
    const client = clientRef.current;
    if (!req || !client) return;
    try {
      await client.request(METHODS.permissionRespond, { request_id: req.request_id, decision, allow_always: allowAlways });
      setPending(null);
      setApprovalError(null);
    } catch (e) { setApprovalError(e instanceof Error ? e.message : String(e)); }
  }, [pending]);
  const close = useCallback(() => { setPending(null); setApprovalError(null); }, []);
  return { pending, approvalError, respond, close };
}

export default function App() {
  const [wsUrl, setWsUrl] = useState(() => resolveWsUrl({ envUrl: envWsUrl }));
  const { modules, status, banner, unreachable } = useKernel(wsUrl);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const { pending, approvalError, respond, close } = useApproval(wsUrl, activeSessionId);
  return (
    <main>
      <h1>AgentJ Dashboard</h1>
      <ConnectionBar
        onConnect={(host, port, token) => {
          const p = port ? Number(port) : undefined;
          const next = resolveWsUrl({
            envUrl: envWsUrl,
            host: host || undefined,
            port: p,
            token: token || undefined,
          });
          setWsUrl(next);
        }}
      />
      {banner && <p>{banner}</p>}
      <p>{status}</p>
      {unreachable && (
        <p>{`agentj dashboard --port ${new URL(resolveWsUrl({})).port}`}</p>
      )}
      <div style={{ display: "flex", gap: "1rem" }}>
        <SessionPanel
          wsUrl={wsUrl}
          activeSessionId={activeSessionId}
          onActiveChange={setActiveSessionId}
        />
        <RunPanel wsUrl={wsUrl} activeSessionId={activeSessionId} />
        <ul>
          {modules.map((m) => (
            <li key={m.id}>
              {m.id}: {m.state}
            </li>
          ))}
        </ul>
      </div>
      <ApprovalDialog request={pending} error={approvalError} onRespond={respond} onClose={close} />
    </main>
  );
}
