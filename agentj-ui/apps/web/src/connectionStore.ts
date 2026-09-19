export type StoredConnection = {
  host: string;
  port: string;
  token: string;
};

const KEY_CONNECTION = "agentj.connection";

function readRaw(): string | null {
  try {
    return localStorage.getItem(KEY_CONNECTION);
  } catch {
    return null;
  }
}

/** Last manual connection from the settings dialog; null when none was saved. */
export function loadConnection(): StoredConnection | null {
  const raw = readRaw();
  if (raw == null) return null;
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== "object") return null;
    const { host, port, token } = parsed as Partial<StoredConnection>;
    if (typeof host !== "string" || !host) return null;
    if (typeof port !== "string" || typeof token !== "string") return null;
    return { host, port, token };
  } catch {
    return null;
  }
}

/** Persist manual connection settings so a page reload reconnects to the same kernel. */
export function saveConnection(conn: StoredConnection): void {
  try {
    localStorage.setItem(KEY_CONNECTION, JSON.stringify(conn));
  } catch {}
}
