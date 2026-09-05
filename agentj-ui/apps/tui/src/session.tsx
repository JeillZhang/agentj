import { useCallback, useEffect, useState } from "react";
import { useKeyboard } from "@opentui/react";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { METHODS } from "@agentj/sdk";
import type { RpcClient } from "@agentj/sdk";

export function getConfigDir(): string {
  const xdg = process.env.XDG_CONFIG_HOME;
  if (xdg && xdg.length > 0) return path.join(xdg, "agentj");
  try {
    return path.join(os.homedir(), ".config", "agentj");
  } catch {
    return path.join("/tmp", "agentj");
  }
}

export function getHistoryPath(): string {
  return path.join(getConfigDir(), "session-history.json");
}

export function loadSessionHistory(): string[] {
  try {
    const p = getHistoryPath();
    if (!fs.existsSync(p)) return [];
    const raw = fs.readFileSync(p, "utf8");
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter((v): v is string => typeof v === "string");
  } catch {
    return [];
  }
}

export function saveSessionHistory(ids: string[]): void {
  try {
    const p = getHistoryPath();
    fs.mkdirSync(path.dirname(p), { recursive: true });
    fs.writeFileSync(p, JSON.stringify(ids, null, 2), "utf8");
  } catch {}
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

export function useSession(client: RpcClient | null) {
  const createNew = useCallback(async (): Promise<string> => {
    if (!client) throw new Error("not connected");
    const res: unknown = await client.request(METHODS.sessionNew, {});
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
      await client.request(METHODS.sessionStatus, { session_id: sessionId });
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

  return { createNew, continueSession, archiveSession };
}
