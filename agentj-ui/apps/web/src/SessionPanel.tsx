import { useCallback, useEffect, useState } from "react";
import {
  METHODS,
  ResponseRouter,
  RpcClient,
  WebSocketTransport,
} from "@agentj/sdk";

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

async function runNewSession(
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

async function runContinue(
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
    return runNewSession(
      wsUrl,
      setFavorites,
      setError,
      setBusy,
      setInput,
      onActiveChange,
    );
  }, [wsUrl, onActiveChange]);

  const continueSession = useCallback(
    (targetId?: string) => {
      return runContinue(
        wsUrl,
        targetId ?? input,
        favorites,
        setFavorites,
        setStale,
        setError,
        setBusy,
        setInput,
        onActiveChange,
      );
    },
    [wsUrl, input, favorites, onActiveChange],
  );

  return { input, setInput, favorites, stale, error, busy, newSession, continueSession };
}

type Props = {
  wsUrl: string;
  activeSessionId: string | null;
  onActiveChange: (id: string | null) => void;
};

export default function SessionPanel({
  wsUrl,
  activeSessionId,
  onActiveChange,
}: Props) {
  const { input, setInput, favorites, stale, error, busy, newSession, continueSession } =
    usePanelState(wsUrl, onActiveChange);

  return (
    <section aria-label="sessions">
      <button type="button" onClick={newSession} disabled={busy}>
        New session
      </button>
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
    </section>
  );
}
