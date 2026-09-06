import { useEffect, useMemo, useRef, useState } from "react";
import { Pause, Play } from "lucide-react";
import type { RunEntry } from "./RunPanel.tsx";

export type RawEvent = {
  session_id: string;
  seq: number;
  ts?: number;
  value: Record<string, unknown>;
};
export type TrajNode =
  | { kind: "user"; seq: number; ts?: number; text: string }
  | { kind: "thinking"; seq: number; ts?: number; text: string }
  | { kind: "tool"; seq: number; ts?: number; id: string; name: string; args: unknown; state: "running" | "ok"; output?: string }
  | { kind: "approval"; seq: number; ts?: number; tool: string; decision: string }
  | { kind: "usage"; seq: number; ts?: number; usage: { prompt_tokens: number; completion_tokens: number } }
  | { kind: "error"; seq: number; ts?: number; message: string };

function isRecord(v: unknown): v is Record<string, unknown> {
  return v !== null && typeof v === "object";
}

export function historyToEntries(events: RawEvent[]): TrajNode[] {
  const results = new Map<string, string>();
  for (const ev of events) {
    const v = ev.value;
    if (v.kind === "tool_result" && typeof v.tool_call_id === "string" && typeof v.content === "string") {
      results.set(v.tool_call_id, v.content);
    }
  }
  const out: TrajNode[] = [];
  for (const ev of events) {
    const v = ev.value;
    switch (v.kind) {
      case "user":
        out.push({ kind: "user", seq: ev.seq, ts: ev.ts, text: typeof v.content === "string" ? v.content : "" });
        break;
      case "thinking":
        out.push({ kind: "thinking", seq: ev.seq, ts: ev.ts, text: typeof v.content === "string" ? v.content : "" });
        break;
      case "tool_call": {
        const tc = v.tool_call;
        const id = isRecord(tc) && typeof tc.id === "string" ? tc.id : "";
        const name = isRecord(tc) && typeof tc.name === "string" ? tc.name : "";
        const output = results.get(id);
        out.push({
          kind: "tool",
          seq: ev.seq,
          ts: ev.ts,
          id,
          name,
          args: isRecord(tc) ? tc.arguments ?? null : null,
          state: output === undefined ? "running" : "ok",
          ...(output === undefined ? {} : { output }),
        });
        break;
      }
      case "tool_result": {
        if ([...results.keys()].some((k) => k === v.tool_call_id) && out.some((n) => n.kind === "tool" && n.id === v.tool_call_id)) break;
        out.push({
          kind: "tool",
          seq: ev.seq,
          ts: ev.ts,
          id: String(v.tool_call_id ?? ""),
          name: "tool",
          args: null,
          state: "ok",
          output: typeof v.content === "string" ? v.content : "",
        });
        break;
      }
      case "permission":
        out.push({
          kind: "approval",
          seq: ev.seq,
          ts: ev.ts,
          tool: typeof v.tool === "string" ? v.tool : "",
          decision: typeof v.decision === "string" ? v.decision : "",
        });
        break;
      case "usage": {
        const u = v.usage;
        out.push({
          kind: "usage",
          seq: ev.seq,
          ts: ev.ts,
          usage: isRecord(u)
            ? {
                prompt_tokens: typeof u.prompt_tokens === "number" ? u.prompt_tokens : 0,
                completion_tokens: typeof u.completion_tokens === "number" ? u.completion_tokens : 0,
              }
            : { prompt_tokens: 0, completion_tokens: 0 },
        });
        break;
      }
      case "error":
        out.push({ kind: "error", seq: ev.seq, ts: ev.ts, message: typeof v.message === "string" ? v.message : "" });
        break;
    }
  }
  return out;
}

export function historyToConversation(events: RawEvent[]): RunEntry[] {
  const entries: RunEntry[] = [];
  const pendingTools = new Map<string, number>();
  for (const ev of events) {
    const v = ev.value;
    switch (v.kind) {
      case "user":
        entries.push({ kind: "user", text: typeof v.content === "string" ? v.content : "" });
        break;
      case "thinking":
        entries.push({ kind: "ai", text: typeof v.content === "string" ? v.content : "" });
        break;
      case "tool_call": {
        const tc = v.tool_call;
        const id = isRecord(tc) && typeof tc.id === "string" ? tc.id : "";
        const name = isRecord(tc) && typeof tc.name === "string" ? tc.name : "";
        const entry: RunEntry = {
          kind: "tool",
          id,
          name,
          args: isRecord(tc) ? tc.arguments ?? null : null,
          state: "running",
        };
        entries.push(entry);
        if (id) pendingTools.set(id, entries.length - 1);
        break;
      }
      case "tool_result": {
        const callId = typeof v.tool_call_id === "string" ? v.tool_call_id : String(v.tool_call_id ?? "");
        const content = typeof v.content === "string" ? v.content : "";
        const idx = callId ? pendingTools.get(callId) : undefined;
        if (idx !== undefined) {
          const e = entries[idx] as Extract<RunEntry, { kind: "tool" }>;
          if (e.state === "running") {
            entries[idx] = { ...e, state: "ok", output: content };
          } else if (e.state === "denied") {
          } else {
            entries[idx] = { ...e, state: "ok", output: content };
          }
          pendingTools.delete(callId);
        } else {
          const runningIdx = entries.findLastIndex((e) => e.kind === "tool" && (e as { state: string }).state === "running");
          if (runningIdx >= 0) {
            const e = entries[runningIdx] as Extract<RunEntry, { kind: "tool" }>;
            entries[runningIdx] = { ...e, state: "ok", output: content };
          } else {
            entries.push({
              kind: "tool",
              id: callId,
              name: "tool",
              args: null,
              state: "ok",
              output: content,
            });
          }
        }
        break;
      }
      case "permission": {
        if (v.decision !== "deny") break;
        let idx = entries.findLastIndex((e) => e.kind === "tool" && (e as { state: string }).state === "running" && (e as { name: string }).name === v.tool);
        if (idx < 0) {
          idx = entries.findLastIndex((e) => e.kind === "tool" && (e as { name: string }).name === v.tool);
        }
        if (idx >= 0) {
          const e = entries[idx] as Extract<RunEntry, { kind: "tool" }>;
          entries[idx] = { ...e, state: "denied" } as RunEntry;
          if (e.id) pendingTools.delete(e.id);
        }
        break;
      }
      case "usage": {
        const u = v.usage;
        if (isRecord(u)) {
          entries.push({
            kind: "usage",
            prompt_tokens: typeof u.prompt_tokens === "number" ? u.prompt_tokens : 0,
            completion_tokens: typeof u.completion_tokens === "number" ? u.completion_tokens : 0,
          });
        }
        break;
      }
    }
  }
  return entries;
}

export function filterNodes(nodes: TrajNode[], query: string, kinds: string[]): TrajNode[] {
  const q = query.trim().toLowerCase();
  return nodes.filter((n) => {
    if (kinds.length > 0 && !kinds.includes(n.kind)) return false;
    if (q === "") return true;
    const text =
      n.kind === "user" || n.kind === "thinking"
        ? n.text
        : n.kind === "tool"
          ? `${n.name} ${JSON.stringify(n.args ?? {})} ${n.output ?? ""}`
          : n.kind === "approval"
            ? `${n.tool} ${n.decision}`
            : n.kind === "usage"
              ? `${n.usage.prompt_tokens} ${n.usage.completion_tokens}`
              : n.message;
    return text.toLowerCase().includes(q);
  });
}

export const NODE_KINDS = ["user", "thinking", "tool", "approval", "usage", "error"] as const;

export function Trajectory({ events }: { events: RawEvent[] }) {
  const nodes = useMemo(() => filterNodes(historyToEntries(events), "", []), [events]);
  const [query, setQuery] = useState("");
  const [kinds, setKinds] = useState<string[]>([]);
  const [playing, setPlaying] = useState(false);
  const [step, setStep] = useState(0);
  const shown = useMemo(() => filterNodes(nodes, query, kinds), [nodes, query, kinds]);
  const playingRef = useRef(playing);
  playingRef.current = playing;

  useEffect(() => {
    if (!playing) return;
    if (step >= shown.length) {
      setPlaying(false);
      return;
    }
    const t = setTimeout(() => {
      if (step + 1 >= shown.length) {
        setPlaying(false);
      }
      setStep((s) => Math.min(s + 1, shown.length));
    }, 700);
    return () => clearTimeout(t);
  }, [playing, step, shown.length]);

  const current = shown[Math.min(step, Math.max(shown.length - 1, 0))];
  return (
    <section aria-label="trajectory" className="aj-card">
      <div className="aj-toolbar">
        <input
          aria-label="search trajectory"
          placeholder="Search…"
          className="aj-input"
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setStep(0);
          }}
        />
        <div className="aj-chips" role="group" aria-label="kind filters">
          {NODE_KINDS.map((k) => (
            <button
              key={k}
              type="button"
              className={kinds.includes(k) ? "aj-chip aj-chip--on" : "aj-chip"}
              onClick={() => setKinds((ks) => (ks.includes(k) ? ks.filter((x) => x !== k) : [...ks, k]))}
            >
              {k}
            </button>
          ))}
          {NODE_KINDS.length > 5 && (
            <span className="aj-chip aj-chip--overflow" aria-label={`+${NODE_KINDS.length - 5} more`}>+{NODE_KINDS.length - 5}</span>
          )}
        </div>
        <button
          type="button"
          className="aj-button--ghost"
          aria-label={playing ? "pause play" : "play"}
          onClick={() => {
            if (!playing && step >= shown.length) setStep(0);
            setPlaying(!playing);
          }}
        >
          {playing ? (
            <>
              <Pause size={16} aria-hidden="true" /> pause
            </>
          ) : (
            <>
              <Play size={16} aria-hidden="true" /> play
            </>
          )}
        </button>
      </div>
      {shown.length === 0 ? (
        <p className="aj-empty">Select a session or no trajectory yet — start a run</p>
      ) : (
        <ol className="aj-traj" aria-label="trajectory steps">
          {shown.map((n, idx) => (
            <li
              key={n.seq}
              className={current?.seq === n.seq && playing ? "aj-traj-step aj-traj-step--cur" : "aj-traj-step"}
              aria-label={`step ${idx + 1} of ${shown.length}`}
            >
              <span className="aj-traj-dot" aria-hidden="true" />
              <span className="aj-traj-seq">{n.seq}</span>
              <TrajNodeView node={n} />
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}

export function TrajNodeView({ node }: { node: TrajNode }) {
  const [open, setOpen] = useState(false);
  const title =
    node.kind === "user"
      ? node.text
      : node.kind === "thinking"
        ? node.text
        : node.kind === "tool"
          ? `${node.name} ${node.state === "ok" ? "ok" : "running"}`
          : node.kind === "approval"
            ? `${node.tool} ${node.decision}`
            : node.kind === "usage"
              ? `in=${node.usage.prompt_tokens} out=${node.usage.completion_tokens}`
              : node.message;
  const details =
    node.kind === "tool" ? JSON.stringify({ args: node.args, output: node.output ?? null }, null, 2) : node.kind === "approval" ? node.decision : null;
  return (
    <div className={node.kind === "error" ? "aj-traj-row aj-traj-row--error" : "aj-traj-row"} data-kind={node.kind}>
      <span className="aj-traj-title">{title}</span>
      {details !== null && (
        <button type="button" className="aj-chip" aria-label="expand node" onClick={() => setOpen(!open)}>
          {open ? "collapse" : "details"}
        </button>
      )}
      {open && details !== null && <pre className="aj-tool-output">{details}</pre>}
    </div>
  );
}
