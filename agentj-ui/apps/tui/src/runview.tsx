import { useEffect, useState, useRef } from "react";
import { METHODS } from "@agentj/sdk";
import type { RpcClient } from "@agentj/sdk";
import type { ProgressFrame } from "@agentj/sdk";
import type { ScrollBoxRenderable } from "@opentui/core";
import { useTheme, type ThemePalette } from "./theme.tsx";

export type RunViewProps = {
  frames: string[];
};

export type RunUsage = {
  prompt_tokens: number;
  completion_tokens: number;
};

export type UsageInfo = RunUsage;

export function formatUsage(usage?: UsageInfo | null): string {
  if (!usage) return "usage: n/a";
  return `in=${usage.prompt_tokens} out=${usage.completion_tokens}`;
}

export type RunViewWithUsageProps = {
  frames: string[];
  usage?: RunUsage | null;
};

function extractText(params: unknown): string | null {
  if (typeof params === "string") return params;
  if (params !== null && typeof params === "object") {
    const obj = params as Record<string, unknown>;
    const direct = obj.text ?? obj.content ?? obj.message ?? obj.value;
    if (typeof direct === "string") return direct;
    const nested = obj.value as Record<string, unknown> | undefined;
    if (nested && typeof nested.text === "string") return nested.text;
    if (nested && typeof nested.content === "string") return nested.content;
    try {
      const s = JSON.stringify(params);
      return s.length > 400 ? `${s.slice(0, 400)}…` : s;
    } catch {
      return null;
    }
  }
  return null;
}

export function useProgressFeed(client: RpcClient | null, onFrame: (t: string) => void): void {
  useEffect(() => {
    if (!client) return;
    client.setNotificationHandler((method, params) => {
      if (method !== METHODS.progress) return;
      const text = extractText(params);
      if (text !== null && text.length > 0) onFrame(text);
    });
  }, [client, onFrame]);
}

export function useRunFrames(client: RpcClient | null): string[] {
  const [frames, setFrames] = useState<string[]>([]);
  useProgressFeed(client, (t) => {
    setFrames((prev) => [...prev, t]);
  });
  return frames;
}

function useAutoScroll(items: { length: number }, ref: React.RefObject<ScrollBoxRenderable | null>): void {
  useEffect(() => {
    const target = ref.current;
    if (!target) return;
    try {
      const max = Math.max(0, items.length * 2);
      target.scrollY = max;
    } catch {}
  }, [items, ref]);
}

export function RunView(props: RunViewProps): React.ReactNode {
  const { frames } = props;
  const scrollRef = useRef<ScrollBoxRenderable>(null);
  useAutoScroll(frames, scrollRef);
  return (
    <box flexDirection="column" flexGrow={1} padding={1}>
      <scrollbox ref={scrollRef} flexGrow={1} style={{ flexGrow: 1 }}>
        {frames.map((f, i) => (
          <text key={i}>{f}</text>
        ))}
      </scrollbox>
    </box>
  );
}

export function RunViewWithUsage(props: RunViewWithUsageProps): React.ReactNode {
  const { frames, usage } = props;
  const scrollRef = useRef<ScrollBoxRenderable>(null);
  useAutoScroll(frames, scrollRef);
  return (
    <box flexDirection="column" flexGrow={1} padding={1}>
      <scrollbox ref={scrollRef} flexGrow={1} style={{ flexGrow: 1 }}>
        {frames.map((f, i) => (
          <text key={i}>{f}</text>
        ))}
      </scrollbox>
      <text>{formatUsage(usage)}</text>
    </box>
  );
}

export type TranscriptRow =
  | { key: number; kind: "text"; text: string }
  | { key: number; kind: "user"; text: string }
  | { key: number; kind: "tool"; text: string; toolId: string; state: "running" | "ok" | "error" }
  | { key: number; kind: "approval"; text: string }
  | { key: number; kind: "usage"; text: string; usage: RunUsage };

export function nextKey(rows: TranscriptRow[]): number {
  return rows.length > 0 ? rows[rows.length - 1].key + 1 : 1;
}

export function rowsFromFrame(rows: TranscriptRow[], frame: ProgressFrame): TranscriptRow[] {
  const next = rows.slice();
  const key = nextKey(rows);
  switch (frame.kind) {
    case "text":
      next.push({ key, kind: "text", text: frame.text });
      break;
    case "tool_call": {
      next.push({ key, kind: "tool", text: frame.name, toolId: frame.id, state: "running" });
      break;
    }
    case "tool_result": {
      const content = frame.content;
      const failed = content.startsWith("Error:");
      const idx = next.findLastIndex(
        (r) => r.kind === "tool" && r.toolId === frame.call_id && r.state === "running",
      );
      if (idx === -1) {
        next.push({
          key,
          kind: "tool",
          text: content.slice(0, 120),
          toolId: frame.call_id,
          state: failed ? "error" : "ok",
        });
      } else {
        const row = next[idx];
        if (row.kind === "tool") {
          next[idx] = {
            ...row,
            state: failed ? "error" : "ok",
            text: failed ? content.slice(0, 120) : row.text,
          };
        }
      }
      break;
    }
    case "usage":
      next.push({ key, kind: "usage", text: formatUsage(frame.usage), usage: frame.usage });
      break;
    case "permission":
      next.push({ key, kind: "approval", text: `${frame.decision} ${frame.tool}` });
      break;
    case "unknown":
      next.push({
        key,
        kind: "text",
        text:
          typeof frame.raw === "string"
            ? frame.raw.slice(0, 200)
            : JSON.stringify(frame.raw).slice(0, 200),
      });
      break;
  }
  return next;
}

export function rowGlyph(row: TranscriptRow): string | null {
  if (row.kind === "approval") return "△";
  if (row.kind === "tool") return row.state === "ok" ? "✓" : row.state === "error" ? "✗" : "│";
  return null;
}

function rowColor(row: TranscriptRow, palette: ThemePalette): string {
  switch (row.kind) {
    case "user":
      return palette.accent;
    case "tool":
      return row.state === "error"
        ? palette.error
        : row.state === "running"
          ? palette.textMuted
          : palette.text;
    case "approval":
      return palette.warning;
    case "usage":
      return palette.textMuted;
    default:
      return palette.text;
  }
}

export function RunTranscript(props: { rows: TranscriptRow[] }): React.ReactNode {
  const { palette } = useTheme();
  const { rows } = props;
  const scrollRef = useRef<ScrollBoxRenderable>(null);
  useAutoScroll(rows, scrollRef);
  return (
    <box flexDirection="column" flexGrow={1} padding={1}>
      <scrollbox ref={scrollRef} flexGrow={1} style={{ flexGrow: 1 }}>
        {rows.map((r) => {
          const glyph = rowGlyph(r);
          return (
            <text key={r.key} style={{ fg: rowColor(r, palette) }}>
              {glyph ? `${glyph} ` : ""}
              {r.text}
            </text>
          );
        })}
      </scrollbox>
    </box>
  );
}
