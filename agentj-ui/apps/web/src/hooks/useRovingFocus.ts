import { useCallback, useState } from "react";

/** Roving-focus selection state for a flat list of `count` rows. */
export interface RovingFocus {
  /** Currently selected row index, or -1 when nothing is selected. */
  selected: number;
  /** Select a row, clamped into `[0, count - 1]` (-1 when the list is empty). */
  setSelected: (index: number) => void;
  /** Move the selection by `delta` rows, clamped at both ends. */
  moveBy: (delta: number) => void;
}

/** Options controlling how a selection index is moved. */
export interface MoveSelectionOptions {
  /** Wrap past either end instead of clamping (default false). */
  wrap?: boolean;
}

/**
 * Move a selection index by `delta`, clamped to `[0, count - 1]` by default.
 * An empty list (`count <= 0`) is always unselected (-1); a negative
 * `current` enters the list at index 0. With `{ wrap: true }` the index
 * wraps around past either end instead of clamping.
 */
export function moveSelection(
  current: number,
  delta: number,
  count: number,
  options?: MoveSelectionOptions,
): number {
  if (count <= 0) return -1;
  const base = current < 0 ? 0 : current;
  if (options?.wrap) return (base + delta + count) % count;
  return Math.min(count - 1, Math.max(0, base + delta));
}

/** First selectable index (`0`), or -1 when the list is empty. */
export function firstIndex(count: number): number {
  return count > 0 ? 0 : -1;
}

/** Last selectable index (`count - 1`), or -1 when the list is empty. */
export function lastIndex(count: number): number {
  return count > 0 ? count - 1 : -1;
}

/** Roving tabindex: only the selected row participates in the tab order. */
export function tabIndexFor(index: number, selected: number): 0 | -1 {
  return index === selected ? 0 : -1;
}

/** Shared roving-focus state for list/tree navigation (`count` visible rows). */
export function useRovingFocus({
  count,
  wrap = false,
}: {
  count: number;
  wrap?: boolean;
}): RovingFocus {
  const [selected, setSelectedState] = useState(-1);

  const setSelected = useCallback(
    (index: number) => {
      setSelectedState(moveSelection(index, 0, count, { wrap }));
    },
    [count, wrap],
  );

  const moveBy = useCallback(
    (delta: number) => {
      setSelectedState((current) => moveSelection(current, delta, count, { wrap }));
    },
    [count, wrap],
  );

  return { selected, setSelected, moveBy };
}
