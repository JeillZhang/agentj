// agentj-ui/packages/sdk/src/modelMeta.ts

/**
 * Compact label for a model's context window (`ModelEntry.context_window`), or `null` when the
 * provider publishes none — callers render nothing rather than a fake number.
 */
export function formatContextWindow(tokens?: number | null): string | null {
  if (typeof tokens !== "number" || !Number.isFinite(tokens) || tokens <= 0) return null;
  if (tokens < 1000) return String(Math.round(tokens));
  if (tokens < 1_000_000) return `${Math.round(tokens / 1000)}k`;
  return `${(tokens / 1_000_000).toFixed(1)}M`;
}
