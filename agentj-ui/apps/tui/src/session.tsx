import { useCallback, useEffect, useState } from "react";
import { useKeyboard } from "@opentui/react";
import * as path from "node:path";
import { METHODS } from "@agentj/sdk";
import type { RpcClient } from "@agentj/sdk";
import { getConfigDir, readJsonFile, writeJsonFile } from "./config.ts";

export { getConfigDir };

export function getHistoryPath(): string {
  return path.join(getConfigDir(), "session-history.json");
}

export function loadSessionHistory(): string[] {
  const parsed = readJsonFile<unknown>("session-history.json");
  if (!Array.isArray(parsed)) return [];
  return parsed.filter((v): v is string => typeof v === "string");
}

export function saveSessionHistory(ids: string[]): void {
  writeJsonFile("session-history.json", ids);
}

export function addToHistory(ids: string[], id: string): string[] {
  const filtered = ids.filter((v) => v !== id);
  return [id, ...filtered].slice(0, 20);
}

export type SessionListProps = {
  history: string[];
  onSelect: (id: string) => void;
  onNew?: () => void;
  activeId?: string | null;
};

function useListNav(
  count: number,
  idx: number,
  setIdx: (fn: (v: number) => number) => void,
  history: string[],
  onSelect: (id: string) => void,
  onNew?: () => void,
): void {
  const hasNew = typeof onNew === "function";
  useKeyboard((key) => {
    const n = key.name.toLowerCase();
    const s = key.sequence;
    if (n === "up" || n === "k" || s === "\u001B[A") setIdx((v) => Math.max(0, v - 1));
    else if (n === "down" || n === "j" || s === "\u001B[B") setIdx((v) => Math.min(count - 1, v + 1));
    else if (n === "return" || n === "enter" || s === "\r" || s === "\n") {
      if (hasNew && idx === history.length) onNew?.();
      else if (history[idx]) onSelect(history[idx]);
    } else if ((n === "n" || n === "N") && hasNew) onNew?.();
  });
}

export function SessionList(props: SessionListProps) {
  const { history, onSelect, onNew, activeId } = props;
  const [idx, setIdx] = useState(() => {
    if (activeId) {
      const pos = history.indexOf(activeId);
      return pos >= 0 ? pos : 0;
    }
    return 0;
  });
  const hasNew = typeof onNew === "function";
  const count = hasNew ? history.length + 1 : history.length;
  useEffect(() => {
    if (idx >= count) setIdx(Math.max(0, count - 1));
  }, [count, idx]);
  useListNav(count, idx, setIdx, history, onSelect, onNew);
  if (history.length === 0 && !hasNew) {
    return (
      <box flexDirection="column" padding={1}>
        <text>No sessions</text>
      </box>
    );
  }
  return (
    <box flexDirection="column" padding={1}>
      <text>Sessions</text>
      {history.map((id, i) => (
        <text key={id}>
          {i === idx ? "▶ " : "  "}
          {id}
          {activeId === id ? " (active)" : ""}
        </text>
      ))}
      {hasNew ? <text>{idx === history.length ? "▶ " : "  "}[New session]</text> : null}
    </box>
  );
}

// v0.22.0 additive project_root; a legacy kernel reply omits it → null (caller falls back).
function projectRootOf(res: unknown): string | null {
  if (res !== null && typeof res === "object" && "project_root" in res) {
    const root = (res as { project_root?: unknown }).project_root;
    if (typeof root === "string" && root.length > 0) return root;
  }
  return null;
}

export function useSession(client: RpcClient | null) {
  const [projectRoot, setProjectRoot] = useState<string | null>(null);

  const createNew = useCallback(async (): Promise<string> => {
    if (!client) throw new Error("not connected");
    const res: unknown = await client.request(METHODS.sessionNew, {});
    setProjectRoot(projectRootOf(res));
    if (typeof res === "string") return res;
    if (res !== null && typeof res === "object" && "session_id" in res) {
      const cand = res.session_id;
      if (typeof cand === "string") return cand;
    }
    if (res !== null && typeof res === "object" && "id" in res) {
      const cand = res.id;
      if (typeof cand === "string") return cand;
    }
    return String(res);
  }, [client]);

  const continueSession = useCallback(
    async (sessionId: string): Promise<string> => {
      if (!client) throw new Error("not connected");
      const status: unknown = await client.request(METHODS.sessionStatus, { session_id: sessionId });
      setProjectRoot(projectRootOf(status));
      await client.request(METHODS.sessionSubscribe, { session_id: sessionId });
      return sessionId;
    },
    [client],
  );

  const archiveSession = useCallback(
    async (sessionId: string): Promise<void> => {
      if (!client) throw new Error("not connected");
      await client.request(METHODS.sessionArchive, { session_id: sessionId });
    },
    [client],
  );

  return { createNew, continueSession, archiveSession, projectRoot };
}
