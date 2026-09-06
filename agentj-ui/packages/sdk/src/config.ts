export const VERSION = "0.13.0";
export const PROTOCOL_VERSION = "0.12.1";

export function resolveWsUrl(opts: { envUrl?: string; host?: string; port?: number; token?: string }): string {
  if (opts.envUrl) {
    if (!opts.token) return opts.envUrl;
    const sep = opts.envUrl.includes("?") ? "&" : "?";
    return `${opts.envUrl}${sep}token=${encodeURIComponent(opts.token)}`;
  }
  const host = opts.host ?? "127.0.0.1";
  const port = opts.port ?? 4096;
  const base = `ws://${host}:${port}/ws`;
  return opts.token ? `${base}?token=${encodeURIComponent(opts.token)}` : base;
}
