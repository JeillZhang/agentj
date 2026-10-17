export const VERSION = "0.29.10";
export const PROTOCOL_VERSION = "0.18.0";

/** Page-origin inputs used to derive a default WS URL in browsers. */
export interface WsLocation {
  hostname: string;
  protocol: string;
}

function deriveHost(location?: WsLocation): string | undefined {
  if (!location) return undefined;
  if (location.protocol !== "http:" && location.protocol !== "https:") return undefined;
  const host = location.hostname;
  if (!host || host === "tauri.localhost") return undefined; // Tauri desktop shares this web build
  return host;
}

function deriveScheme(location?: WsLocation): "ws" | "wss" {
  // The desktop build stays on plaintext loopback even if it ever serves over https.
  if (location?.hostname === "tauri.localhost") return "ws";
  return location?.protocol === "https:" ? "wss" : "ws";
}

export function resolveWsUrl(opts: {
  envUrl?: string;
  host?: string;
  port?: number;
  token?: string;
  location?: WsLocation;
}): string {
  if (opts.envUrl) {
    if (!opts.token) return opts.envUrl;
    const sep = opts.envUrl.includes("?") ? "&" : "?";
    return `${opts.envUrl}${sep}token=${encodeURIComponent(opts.token)}`;
  }
  const host = opts.host ?? deriveHost(opts.location) ?? "127.0.0.1";
  const port = opts.port ?? 4096;
  const scheme = deriveScheme(opts.location);
  const base = `${scheme}://${host}:${port}/ws`;
  return opts.token ? `${base}?token=${encodeURIComponent(opts.token)}` : base;
}
