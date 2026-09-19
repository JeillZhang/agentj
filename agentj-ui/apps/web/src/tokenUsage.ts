export type TokenUsage = { prompt: number; completion: number };

/** `< 1000` verbatim; below the 999.95k rounding edge as one-decimal kilo (`.0` trimmed); otherwise one-decimal mega. */
export function formatTokenTotal(n: number): string {
  if (n < 1000) return String(n);
  const k = n / 1000;
  if (k < 999.95) return `${trimTrailingZero(k.toFixed(1))}k`;
  return `${trimTrailingZero((n / 1_000_000).toFixed(1))}M`;
}

function trimTrailingZero(value: string): string {
  return value.endsWith(".0") ? value.slice(0, -2) : value;
}

/**
 * Session readout: total plus input/output breakdown, each through `formatTokenTotal`.
 * Segments are rounded independently and may not visually add up to the rounded total;
 * exact values live in `formatUsageTitle`.
 */
export function formatUsageSummary(usage: TokenUsage): string {
  const total = usage.prompt + usage.completion;
  return `Σ ${formatTokenTotal(total)} · 输入 ${formatTokenTotal(usage.prompt)} · 输出 ${formatTokenTotal(usage.completion)}`;
}

/** Exact-number tooltip/accessible label for the session readout. */
export function formatUsageTitle(usage: TokenUsage): string {
  const total = usage.prompt + usage.completion;
  return `本会话累计：输入 ${usage.prompt.toLocaleString()} · 输出 ${usage.completion.toLocaleString()} · 合计 ${total.toLocaleString()}`;
}
