import { useEffect, useRef, useState } from "react";
import {
  METHODS,
  ResponseRouter,
  RpcClient,
  WebSocketTransport,
} from "@agentj/sdk";

type Props = { wsUrl: string; activeSessionId: string | null };
type Usage = { prompt: number; completion: number };

function extractDelta(value: unknown): string | null {
  if (!value || typeof value !== "object") return null;
  if (!("kind" in value)) return null;
  const obj = value as Record<string, unknown>; // wire ProgressValue
  if (obj.kind !== "text") return null;
  if (typeof obj.delta === "string") return obj.delta;
  if (typeof obj.text === "string") return obj.text;
  return null;
}

async function runSession(
  wsUrl: string,
  sessionId: string,
  content: string,
  onDelta: (d: string) => void,
): Promise<Usage | null> {
  const router = new ResponseRouter();
  const transport = new WebSocketTransport(wsUrl, router);
  const client = new RpcClient(transport, router);
  client.setNotificationHandler((method, params) => {
    if (method !== METHODS.progress) return;
    const p = params as Record<string, unknown>;
    const delta = extractDelta(p.value);
    if (delta) onDelta(delta);
  });
  try {
    const res = (await client.request(METHODS.sessionRun, {
      session_id: sessionId,
      content,
    })) as { usage?: { prompt_tokens: number; completion_tokens: number } } | null;
    if (res?.usage) return { prompt: res.usage.prompt_tokens, completion: res.usage.completion_tokens };
    return null;
  } catch {
    return null;
  } finally {
    transport.close?.();
  }
}

export default function RunPanel({ wsUrl, activeSessionId }: Props) {
  const [input, setInput] = useState("");
  const [frames, setFrames] = useState<string[]>([]);
  const [running, setRunning] = useState(false);
  const [usage, setUsage] = useState<Usage | null>(null);
  const feedRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = feedRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [frames, usage]);
  const send = async () => {
    if (!activeSessionId || !input.trim() || running) return;
    const content = input.trim();
    setInput(""); setFrames([]); setUsage(null); setRunning(true);
    const result = await runSession(wsUrl, activeSessionId, content, (d) => setFrames((p) => [...p, d]));
    if (result) setUsage(result);
    setRunning(false);
  };
  return (
    <section aria-label="run" style={{ flex: 1 }}>
      <div>
        <label htmlFor="aj-prompt">prompt</label>
        <textarea
          id="aj-prompt"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          placeholder="prompt"
          rows={4}
        />
        <button type="button" onClick={send} disabled={running || !activeSessionId}>Send</button>
      </div>
      <div
        ref={feedRef}
        aria-label="progress feed"
        style={{ maxHeight: "320px", overflow: "auto", border: "1px solid #ddd" }}
      >
        {frames.map((f, i) => <div key={i}>{f}</div>)}
        {usage && <div>in={usage.prompt} out={usage.completion}</div>}
      </div>
    </section>
  );
}
