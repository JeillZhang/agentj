export const PROTOCOL_VERSION = "0.11.0";

export function resolveWsUrl(opts: {
  envUrl?: string;
  host?: string;
  port?: number;
  token?: string;
}): string {
  if (opts.envUrl) return opts.envUrl;
  const host = opts.host ?? "127.0.0.1";
  const port = opts.port ?? 4096;
  const base = `ws://${host}:${port}/ws`;
  return opts.token ? `${base}?token=${opts.token}` : base;
}
