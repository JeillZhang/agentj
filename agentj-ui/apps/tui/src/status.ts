import type { TransportStatus } from "@agentj/sdk";

/** Shape of a `provider/usage` window as far as the status line cares. */
export type SubWindowLike = { percent?: number | null } | null | undefined;

/**
 * Whatever the status line needs from the last usage read. `stale` is the caller's verdict
 * (server flag or age beyond the freshness budget) — formatting never calls the clock.
 */
export type SubSuffixInput =
  | { windows?: SubWindowLike[] | null; fetchedAt?: number; stale?: boolean }
  | null
  | undefined;

export type SubTone = "default" | "warning" | "error";
export type SubSuffix = { text: string; tone: SubTone };

const WARN_PERCENT = 70;
const ERROR_PERCENT = 90;

function toneOf(worst: number): SubTone {
  if (worst >= ERROR_PERCENT) return "error";
  if (worst >= WARN_PERCENT) return "warning";
  return "default";
}

function percentsOf(input: SubSuffixInput): number[] {
  const windows = input?.windows ?? [];
  const out: number[] = [];
  for (const w of windows) {
    const p = w?.percent;
    if (typeof p === "number" && Number.isFinite(p)) out.push(p);
  }
  return out;
}

/**
 * Render the usage suffix appended to the connection status line.
 * No credential (no windows) yields an empty suffix, so the line stays untouched.
 */
export function formatSubSuffix(input: SubSuffixInput): SubSuffix {
  const percents = percentsOf(input);
  if (percents.length === 0) return { text: "", tone: "default" };
  const worst = Math.max(...percents);
  const body = percents.map((p) => `${Math.round(p)}%`).join("/");
  const tilde = input?.stale ? "~" : "";
  return { text: `· sub ${tilde}${body}`, tone: toneOf(worst) };
}

export function getStatusLine(
  authError: string | null,
  status: TransportStatus,
  sub?: SubSuffixInput,
): string {
  const base = baseLine(authError, status);
  const { text } = formatSubSuffix(sub);
  // The suffix carries its own separator; only a present suffix earns the space before it.
  return text === "" ? base : `${base} ${text}`;
}

function baseLine(authError: string | null, status: TransportStatus): string {
  if (authError) return `auth error: ${authError}`;
  if (status === "open") return "connected";
  if (status === "closed") return "kernel unreachable";
  return "connecting...";
}
