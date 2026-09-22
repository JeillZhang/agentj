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

/**
 * Move a selection index by `delta`, clamped to `[0, count - 1]`.
 * An empty list (`count <= 0`) is always unselected (-1); a negative
 * `current` enters the list at index 0.
 */
export function moveSelection(current: number, delta: number, count: number): number {
  if (count <= 0) return -1;
  const base = current < 0 ? 0 : current;
  return Math.min(count - 1, Math.max(0, base + delta));
}

/** Roving tabindex: only the selected row participates in the tab order. */
export function tabIndexFor(index: number, selected: number): 0 | -1 {
  return index === selected ? 0 : -1;
}

/** Shared roving-focus state for list/tree navigation (`count` visible rows). */
export function useRovingFocus({ count }: { count: number }): RovingFocus {
  const [selected, setSelectedState] = useState(-1);

  const setSelected = useCallback(
    (index: number) => {
      if (count <= 0) {
        setSelectedState(-1);
        return;
      }
      setSelectedState(Math.min(count - 1, Math.max(0, index)));
    },
    [count],
  );

  const moveBy = useCallback(
    (delta: number) => {
      setSelectedState((current) => moveSelection(current, delta, count));
    },
    [count],
  );

  return { selected, setSelected, moveBy };
}
