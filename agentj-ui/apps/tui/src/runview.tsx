import { useEffect, useState, useRef } from "react";
import { METHODS } from "@agentj/sdk";
import type { RpcClient } from "@agentj/sdk";
import type { ScrollBoxRenderable } from "@opentui/core";

export type RunViewProps = {
  frames: string[];
};

export type RunUsage = {
  input_tokens: number;
  output_tokens: number;
};

export type UsageInfo = RunUsage;

export function formatUsage(usage?: UsageInfo | null): string {
  if (!usage) return "usage: n/a";
  return `in=${usage.input_tokens} out=${usage.output_tokens}`;
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

function useAutoScroll(frames: string[], ref: React.RefObject<ScrollBoxRenderable | null>): void {
  useEffect(() => {
    const target = ref.current;
    if (!target) return;
    try {
      const max = Math.max(0, frames.length * 2);
      target.scrollY = max;
    } catch {}
  }, [frames, ref]);
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
