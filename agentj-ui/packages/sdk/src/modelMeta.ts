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

/** The provider/name pair both pickers identify a model row by. */
export type ModelLike = { provider?: string | null; name: string };

/** Which rule produced a marker row: an explicit pick, a unique default name, or nothing. */
export type MarkerMatch = "explicit" | "default" | "none";

/**
 * A model row is identified by name + provider, because two providers can serve the same
 * model name. The `\u0000` separator keeps `("a", "bx")` from colliding with `("ab", "x")`.
 */
export function modelKey(entry: ModelLike): string {
  return `${entry.provider ?? ""}\u0000${entry.name}`;
}

/**
 * Index of the row a picker should mark, or -1 when no row is authoritative.
 *
 * An explicit pick identifies its own row (by provider + name). Without one, only a
 * provider-agnostic name served by exactly one row can: a name offered by several
 * providers cannot say which provider a run would use, so it marks nothing.
 */
export function resolveModelMarker(
  providers: readonly ModelLike[],
  selected: ModelLike | null | undefined,
  defaultModel: string | null | undefined,
): { index: number; match: MarkerMatch } {
  if (selected) {
    const key = modelKey(selected);
    const index = providers.findIndex((entry) => modelKey(entry) === key);
    if (index >= 0) return { index, match: "explicit" };
  }
  const hits = providers.reduce(
    (acc: number[], entry, index) => (entry.name === defaultModel ? [...acc, index] : acc),
    [],
  );
  return hits.length === 1 ? { index: hits[0], match: "default" } : { index: -1, match: "none" };
}
