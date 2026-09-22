import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

// Shared config-dir resolution for all TUI persistence stores.
// Mirrors the previous local helpers in theme.tsx / sidebar.tsx / session.tsx:
// XDG_CONFIG_HOME wins when set (non-empty), else ~/.config, else /tmp.
export function getConfigDir(): string {
  const xdg = process.env.XDG_CONFIG_HOME;
  if (xdg && xdg.length > 0) return path.join(xdg, "agentj");
  try {
    return path.join(os.homedir(), ".config", "agentj");
  } catch {
    return path.join("/tmp", "agentj");
  }
}

// Reads <configDir>/<name> as JSON. Missing, unreadable or corrupt files are
// silently ignored (null) — persistence must never crash the TUI.
export function readJsonFile<T>(name: string): T | null {
  try {
    const p = path.join(getConfigDir(), name);
    if (!fs.existsSync(p)) return null;
    const raw = fs.readFileSync(p, "utf8");
    return JSON.parse(raw) as T;
  } catch {
    return null;
  }
}

// Writes <configDir>/<name> as 2-space JSON, creating parent dirs as needed.
// Write failures are silently ignored (same semantics as the old stores).
export function writeJsonFile(name: string, value: unknown): void {
  try {
    const p = path.join(getConfigDir(), name);
    fs.mkdirSync(path.dirname(p), { recursive: true });
    fs.writeFileSync(p, JSON.stringify(value, null, 2), "utf8");
  } catch {}
}
