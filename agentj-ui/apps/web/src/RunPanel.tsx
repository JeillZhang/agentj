import { useEffect, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { METHODS, parseProgress, RpcClient } from "@agentj/sdk";
import type { ProgressFrame } from "@agentj/sdk";

export type RunEntry =
  | { kind: "user"; text: string }
  | { kind: "ai"; text: string }
  | { kind: "tool"; id: string; name: string; args: unknown; state: "running" | "ok" | "denied" | "failed"; output?: string }
  | { kind: "usage"; prompt_tokens: number; completion_tokens: number }
  | { kind: "error"; text: string };

export function reduceRunFeed(entries: RunEntry[], frame: ProgressFrame): RunEntry[] {
  const last = entries[entries.length - 1];
  switch (frame.kind) {
    case "text":
      if (last?.kind === "ai") {
        return [...entries.slice(0, -1), { kind: "ai", text: last.text + frame.text }];
      }
      return [...entries, { kind: "ai", text: frame.text }];
    case "tool_call":
      return [
        ...entries,
        { kind: "tool", id: frame.id, name: frame.name, args: frame.arguments, state: "running" },
      ];
    case "tool_result": {
      const settle = (t: Extract<RunEntry, { kind: "tool" }>, j: number): RunEntry[] => {
        const state: "ok" | "failed" = frame.content.startsWith("Error:") ? "failed" : "ok";
        const next: RunEntry = { ...t, state, output: frame.content };
        return [...entries.slice(0, j), next, ...entries.slice(j + 1)];
      };
      const i = entries.findIndex((e) => e.kind === "tool" && e.id === frame.call_id);
      if (i >= 0) {
        const t = entries[i] as Extract<RunEntry, { kind: "tool" }>;
        return t.state === "running" ? settle(t, i) : entries;
      }
      const j = entries.findLastIndex((e) => e.kind === "tool" && e.state === "running");
      if (j < 0) return entries;
      return settle(entries[j] as Extract<RunEntry, { kind: "tool" }>, j);
    }
    case "permission": {
      if (frame.decision !== "deny") return entries;
      const j = entries.findLastIndex(
        (e) => e.kind === "tool" && e.state === "running" && e.name === frame.tool,
      );
      if (j < 0) return entries;
      const t = entries[j] as Extract<RunEntry, { kind: "tool" }>;
      return [...entries.slice(0, j), { ...t, state: "denied" }, ...entries.slice(j + 1)];
    }
    case "usage": {
      const u = { kind: "usage" as const, prompt_tokens: frame.usage.prompt_tokens, completion_tokens: frame.usage.completion_tokens };
      return last?.kind === "usage" ? [...entries.slice(0, -1), u] : [...entries, u];
    }
    case "unknown":
      return entries;
  }
}

export function failRun(entries: RunEntry[], message: string): RunEntry[] {
  const settled = entries.map((e) =>
    e.kind === "tool" && e.state === "running" ? { ...e, state: "failed" as const } : e,
  );
  return [...settled, { kind: "error", text: message }];
}

export function fmtDuration(ms: number): string {
  return ms < 1000 ? `${Math.round(ms)}ms` : `${(ms / 1000).toFixed(2)}s`;
}

type Props = {
  client: RpcClient | null;
  activeSessionId: string | null;
  initialEntries?: RunEntry[];
  pendingTool?: string;
  model?: string;
  elapsedMs?: number;
  wsUrl?: string;
};

export function toolGlyph(entry: Extract<RunEntry, { kind: "tool" }>, pending: boolean) {
  if (entry.state === "ok") return "✓";
  if (entry.state === "denied" || entry.state === "failed") return "✗";
  return pending ? "△" : "│";
}

export function ToolCard({
  entry,
  pending,
}: {
  entry: Extract<RunEntry, { kind: "tool" }>;
  pending: boolean;
}) {
  const [open, setOpen] = useState(false);
  const lines = (entry.output ?? "").split("\n");
  const folded = !open && lines.length > 3;
  const shown = folded ? lines.slice(0, 3).join("\n") : entry.output ?? "";
  const state = pending ? "pending" : entry.state;
  return (
    <section className="aj-tool-card" aria-label={entry.name}>
      <span className="aj-tool-status" data-state={state}>{toolGlyph(entry, pending)}</span>
      <span className="aj-tool-name">{entry.name}</span>
      {entry.output !== undefined && (
        <button type="button" aria-label={open ? "collapse" : "expand"} onClick={() => setOpen(!open)}>
          {open ? "collapse" : "expand"}
        </button>
      )}
      {shown !== "" && <pre className="aj-tool-output">{shown}{folded ? " …" : ""}</pre>}
    </section>
  );
}

export function ConversationFeed({
  entries,
  pendingTool,
  model,
  elapsedMs,
}: {
  entries: RunEntry[];
  pendingTool?: string;
  model?: string;
  elapsedMs?: number;
}) {
  const endRef = useRef<HTMLDivElement | null>(null);
  const [pinned, setPinned] = useState(true);
  useEffect(() => {
    if (pinned) (endRef.current as unknown as { scrollIntoView?: (o?: unknown) => void })?.scrollIntoView?.({ block: "end" });
  });
  return (
    <div
      className="aj-conv"
      role="feed"
      aria-label="progress feed"
      onScroll={(e) => {
        const el = e.currentTarget;
        setPinned(el.scrollHeight - el.scrollTop - el.clientHeight < 40);
      }}
    >
      {entries.map((entry, i) => {
        switch (entry.kind) {
          case "user":
            return (
              <div key={i} className="aj-bubble-user">{entry.text}</div>
            );
          case "ai":
            return (
              <div key={i} className="aj-ai">
                <ReactMarkdown remarkPlugins={[remarkGfm]}>{entry.text}</ReactMarkdown>
              </div>
            );
          case "tool":
            return (
              <ToolCard key={i} entry={entry} pending={pendingTool === entry.name && entry.state === "running"} />
            );
          case "usage": {
            const isLast = i === entries.length - 1;
            return (
              <div key={i} className="aj-meta">
                {model !== undefined && <span className="aj-meta-item">{model}</span>}
                {isLast && elapsedMs !== undefined && (
                  <span className="aj-meta-item">{fmtDuration(elapsedMs)}</span>
                )}
                <span className="aj-meta-item">
                  in={entry.prompt_tokens} out={entry.completion_tokens}
                </span>
              </div>
            );
          }
          case "error":
            return (
              <div key={i} className="aj-error-row" role="alert">
                {entry.text}
              </div>
            );
        }
      })}
      <div ref={endRef} />
    </div>
  );
}

export default function RunPanel({
  client,
  activeSessionId,
  initialEntries = [],
  pendingTool,
  model,
}: Props) {
  const [entries, setEntries] = useState<RunEntry[]>(initialEntries);
  const [elapsedMs, setElapsedMs] = useState<number | undefined>(undefined);
  const [draft, setDraft] = useState("");
  const busy = useRef(false);
  useEffect(() => {
    if (initialEntries.length > 0) setEntries(initialEntries);
  }, [initialEntries]);

  useEffect(() => {
    if (!client) return;
    const handler = (method: string, params: unknown) => {
      if (method !== METHODS.progress) return;
      const f = parseProgress(params);
      if (f.kind === "unknown") {
        console.debug("run:ignored-frame", params);
        return;
      }
      setEntries((prev) => reduceRunFeed(prev, f));
    };
    client.setNotificationHandler(handler);
    return () => {
      client.removeNotificationHandler?.(handler);
    };
  }, [client]);

  async function submit(text: string) {
    if (!activeSessionId || busy.current || text.trim() === "" || !client) return;
    busy.current = true;
    setElapsedMs(undefined);
    const startedAt = performance.now();
    setEntries((prev) => [...prev, { kind: "user", text }]);
    let usage: { usage?: { prompt_tokens?: number; completion_tokens?: number } } | null = null;
    try {
      try {
        usage = (await client.request(METHODS.sessionRun, {
          session_id: activeSessionId,
          content: text,
        })) as { usage?: { prompt_tokens?: number; completion_tokens?: number } } | null;
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        setEntries((prev) => failRun(prev, message));
      }
    } finally {
      setElapsedMs(performance.now() - startedAt);
      busy.current = false;
    }
    const pt = usage?.usage?.prompt_tokens;
    const ct = usage?.usage?.completion_tokens;
    if (typeof pt === "number" && typeof ct === "number") {
      setEntries((prev) => {
        const last = prev[prev.length - 1];
        if (last?.kind === "usage") return prev;
        return [...prev, { kind: "usage", prompt_tokens: pt, completion_tokens: ct }];
      });
    }
  }

  return (
    <section aria-label="run" className="aj-card">
      <ConversationFeed
        entries={entries}
        pendingTool={pendingTool}
        model={model}
        elapsedMs={elapsedMs}
      />
      <div className="aj-composer">
        <label htmlFor="aj-prompt" className="aj-visually-hidden">prompt</label>
        <textarea
          id="aj-prompt"
          className="aj-input"
          value={draft}
          placeholder={activeSessionId ? "继续追问…" : "先新建或选择一个会话"}
          disabled={!activeSessionId || busy.current}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              submit(draft);
              setDraft("");
            }
          }}
        />
            <button
              type="button"
              className="aj-button"
              disabled={!activeSessionId || busy.current}
              onClick={() => {
                submit(draft);
                setDraft("");
              }}
            >
          Send
        </button>
      </div>
    </section>
  );
}
