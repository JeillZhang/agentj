import { useCallback, useEffect, useState } from "react";
import {
  METHODS,
  ResponseRouter,
  RpcClient,
  WebSocketTransport,
} from "@agentj/sdk";

export type ListRow = { id: string; state: string; preview?: string };
const PREVIEW_MAX = 120;
export function rowsFromList(
  sessions: Array<{ session_id?: string; id?: string; state?: string; preview?: string | null }>,
): ListRow[] {
  return (sessions ?? []).map((s) => {
    const preview = s.preview;
    return {
      id: String(s.session_id ?? s.id ?? ""),
      state: s.state ?? "active",
      preview:
        preview == null
          ? undefined
          : preview.length > PREVIEW_MAX
            ? `${preview.slice(0, PREVIEW_MAX - 1)}…`
            : preview,
    };
  });
}

function openOpClient(wsUrl: string) {
  const router = new ResponseRouter();
  const transport = new WebSocketTransport(wsUrl, router);
  const client = new RpcClient(transport, router);
  return { router, transport, client };
}

const FAVORITES_KEY = "agentj.favorites";
const STALE_KEY = "agentj.favorites.stale";

function loadFavorites(): string[] {
  try {
    const raw = localStorage.getItem(FAVORITES_KEY);
    const arr = raw ? (JSON.parse(raw) as unknown) : [];
    return Array.isArray(arr) ? arr.filter((v) => typeof v === "string") : [];
  } catch {
    return [];
  }
}

function loadStale(): Set<string> {
  try {
    const raw = localStorage.getItem(STALE_KEY);
    const arr = raw ? (JSON.parse(raw) as unknown) : [];
    if (!Array.isArray(arr)) return new Set();
    return new Set(arr.filter((v) => typeof v === "string"));
  } catch {
    return new Set();
  }
}

function persistFavorites(ids: string[]): void {
  localStorage.setItem(FAVORITES_KEY, JSON.stringify(ids));
}

function persistStale(ids: Set<string>): void {
  localStorage.setItem(STALE_KEY, JSON.stringify([...ids]));
}

async function runNewSessionWithClient(
  client: RpcClient,
  setFavorites: (u: (p: string[]) => string[]) => void,
  setError: (e: string | null) => void,
  setBusy: (b: boolean) => void,
  setInput: (v: string) => void,
  onActiveChange: (id: string | null) => void,
): Promise<void> {
  setBusy(true);
  setError(null);
  try {
    const res = (await client.request(METHODS.sessionNew, {})) as {
      session_id: string;
    };
    const id = res.session_id;
    setFavorites((prev) => {
      if (prev.includes(id)) return prev;
      const next = [...prev, id];
      persistFavorites(next);
      return next;
    });
    onActiveChange(id);
    setInput(id);
  } catch (e) {
    setError(e instanceof Error ? e.message : String(e));
  } finally {
    setBusy(false);
  }
}

async function runContinueWithClient(
  client: RpcClient,
  rawId: string,
  favorites: string[],
  setFavorites: (u: (p: string[]) => string[]) => void,
  setStale: (u: (p: Set<string>) => Set<string>) => void,
  setError: (e: string | null) => void,
  setBusy: (b: boolean) => void,
  setInput: (v: string) => void,
  onActiveChange: (id: string | null) => void,
): Promise<void> {
  const id = rawId.trim();
  if (!id) return;
  setBusy(true);
  setError(null);
  try {
    await client.request(METHODS.sessionStatus, { session_id: id });
    await client.request(METHODS.sessionSubscribe, { session_id: id });
    setFavorites((prev) => {
      if (prev.includes(id)) return prev;
      const next = [...prev, id];
      persistFavorites(next);
      return next;
    });
    setStale((prev) => {
      if (!prev.has(id)) return prev;
      const next = new Set(prev);
      next.delete(id);
      persistStale(next);
      return next;
    });
    onActiveChange(id);
    setInput(id);
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    setError(msg);
    if (favorites.includes(id)) {
      setStale((prev) => {
        if (prev.has(id)) return prev;
        const next = new Set(prev);
        next.add(id);
        persistStale(next);
        return next;
      });
    }
  } finally {
    setBusy(false);
  }
}

async function runNewSessionFallback(
  wsUrl: string,
  setFavorites: (u: (p: string[]) => string[]) => void,
  setError: (e: string | null) => void,
  setBusy: (b: boolean) => void,
  setInput: (v: string) => void,
  onActiveChange: (id: string | null) => void,
): Promise<void> {
  setBusy(true);
  setError(null);
  try {
    const router = new ResponseRouter();
    const transport = new WebSocketTransport(wsUrl, router);
    const client = new RpcClient(transport, router);
    const res = (await client.request(METHODS.sessionNew, {})) as {
      session_id: string;
    };
    const id = res.session_id;
    setFavorites((prev) => {
      if (prev.includes(id)) return prev;
      const next = [...prev, id];
      persistFavorites(next);
      return next;
    });
    onActiveChange(id);
    setInput(id);
  } catch (e) {
    setError(e instanceof Error ? e.message : String(e));
  } finally {
    setBusy(false);
  }
}

async function runContinueFallback(
  wsUrl: string,
  rawId: string,
  favorites: string[],
  setFavorites: (u: (p: string[]) => string[]) => void,
  setStale: (u: (p: Set<string>) => Set<string>) => void,
  setError: (e: string | null) => void,
  setBusy: (b: boolean) => void,
  setInput: (v: string) => void,
  onActiveChange: (id: string | null) => void,
): Promise<void> {
  const id = rawId.trim();
  if (!id) return;
  setBusy(true);
  setError(null);
  try {
    const router = new ResponseRouter();
    const transport = new WebSocketTransport(wsUrl, router);
    const client = new RpcClient(transport, router);
    await client.request(METHODS.sessionStatus, { session_id: id });
    await client.request(METHODS.sessionSubscribe, { session_id: id });
    setFavorites((prev) => {
      if (prev.includes(id)) return prev;
      const next = [...prev, id];
      persistFavorites(next);
      return next;
    });
    setStale((prev) => {
      if (!prev.has(id)) return prev;
      const next = new Set(prev);
      next.delete(id);
      persistStale(next);
      return next;
    });
    onActiveChange(id);
    setInput(id);
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    setError(msg);
    if (favorites.includes(id)) {
      setStale((prev) => {
        if (prev.has(id)) return prev;
        const next = new Set(prev);
        next.add(id);
        persistStale(next);
        return next;
      });
    }
  } finally {
    setBusy(false);
  }
}

function usePanelState(
  wsUrl: string,
  client: RpcClient | null,
  onActiveChange: (id: string | null) => void,
) {
  const [input, setInput] = useState("");
  const [favorites, setFavorites] = useState<string[]>(() => loadFavorites());
  const [stale, setStale] = useState<Set<string>>(() => loadStale());
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    setFavorites(loadFavorites());
    setStale(loadStale());
  }, []);

  const newSession = useCallback(() => {
    if (client) {
      return runNewSessionWithClient(client, setFavorites, setError, setBusy, setInput, onActiveChange);
    }
    return runNewSessionFallback(wsUrl, setFavorites, setError, setBusy, setInput, onActiveChange);
  }, [wsUrl, client, onActiveChange]);

  const continueSession = useCallback(
    (targetId?: string) => {
      if (client) {
        return runContinueWithClient(client, targetId ?? input, favorites, setFavorites, setStale, setError, setBusy, setInput, onActiveChange);
      }
      return runContinueFallback(wsUrl, targetId ?? input, favorites, setFavorites, setStale, setError, setBusy, setInput, onActiveChange);
    },
    [wsUrl, client, input, favorites, onActiveChange],
  );

  const clearHistory = useCallback(() => {
    try {
      localStorage.removeItem(FAVORITES_KEY);
      localStorage.removeItem(STALE_KEY);
    } catch {}
    setFavorites([]);
    setStale(new Set());
    setInput("");
    setError(null);
    onActiveChange(null);
  }, [onActiveChange]);

  return { input, setInput, favorites, stale, error, busy, newSession, continueSession, clearHistory };
}

type Props = {
  wsUrl: string;
  client?: RpcClient | null;
  activeSessionId: string | null;
  onActiveChange: (id: string | null) => void;
};

export default function SessionPanel({
  wsUrl,
  client: sharedClient = null,
  activeSessionId,
  onActiveChange,
}: Props) {
  const { input, setInput, favorites, stale, error, busy, newSession, continueSession, clearHistory: clearState } =
    usePanelState(wsUrl, sharedClient, onActiveChange);
  const [listRows, setListRows] = useState<ListRow[]>([]);
  const clearHistory = useCallback(() => {
    clearState();
    setListRows([]);
  }, [clearState]);
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        if (sharedClient) {
          const res = (await sharedClient.request(METHODS.sessionList, {})) as {
            sessions?: Array<{ session_id?: string; id?: string; state?: string; preview?: string | null }>;
          } | null;
          if (!cancelled) setListRows(rowsFromList(res?.sessions ?? []));
          return;
        }
        const { transport, client } = openOpClient(wsUrl);
        try {
          const res = (await client.request(METHODS.sessionList, {})) as {
            sessions?: Array<{ session_id?: string; id?: string; state?: string; preview?: string | null }>;
          } | null;
          if (!cancelled) setListRows(rowsFromList(res?.sessions ?? []));
        } finally {
          (transport as unknown as { close?: () => void }).close?.();
        }
      } catch (err) {
        if (!cancelled) {
          console.debug("session-list", err);
          setListRows([]);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [wsUrl, sharedClient]);

  return (
    <section aria-label="sessions">
      <div style={{ display: "flex", gap: 8 }}>
        <button type="button" onClick={newSession} disabled={busy}>
          New session
        </button>
        <button type="button" onClick={clearHistory} disabled={busy} aria-label="clear history">
          Clear
        </button>
      </div>
      <div>
        <label htmlFor="aj-session-id">session id</label>
        <input
          id="aj-session-id"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          placeholder="session id"
        />
        <button type="button" onClick={() => continueSession()} disabled={busy}>
          Continue
        </button>
      </div>
      {error && (
        <div role="alert">
          <p>{error}</p>
          <button type="button" onClick={newSession}>
            New
          </button>
        </div>
      )}
      {activeSessionId && <p>active: {activeSessionId}</p>}
      {favorites.length > 0 && (
        <ul aria-label="favorites">
          {favorites.map((id) => (
            <li key={id}>
              <button type="button" onClick={() => continueSession(id)}>
                {stale.has(id) ? `${id} (stale)` : id}
              </button>
            </li>
          ))}
        </ul>
      )}
      <h3>Recent sessions</h3>
      <ul aria-label="session list" className="aj-session-list">
        {listRows.map((r) => (
          <li key={r.id}>
            <button type="button" className="aj-session-row" onClick={() => continueSession(r.id)}>
              <span className="aj-session-state" data-state={r.state} />
              <span className="aj-session-id">{r.id}</span>
              {r.preview !== undefined && <span className="aj-session-preview">{r.preview}</span>}
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}
