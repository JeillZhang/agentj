/** Scroll-driven composer collapse state machine (pure, no DOM). */

export const COLLAPSE_DISTANCE = 48;
export const EXPAND_DISTANCE = 40;
export const COLLAPSE_LOCK_MS = 260;

export type ScrollDirection = "up" | "down" | "none";

export type ScrollSignal = {
  /** Distance from the scroll bottom in px (caller clamps to >= 0). */
  distance: number;
  direction: ScrollDirection;
  /** True when the scroll was issued by our own scrollIntoView auto-follow. */
  programmatic: boolean;
};

// Session switches reset the flag through the existing `key={activeSessionId}`
// remount, so no explicit reset event is needed.
export type ComposerCollapseEvent =
  | { type: "scroll-away" }
  | { type: "reach-bottom" }
  | { type: "expand-click" }
  | { type: "question-appeared" };

export type ComposerCollapseContext = { hasPendingQuestion: boolean };

/** Maps a scroll signal to a state-machine event (8px hysteresis band: 40 / 48). */
export function classifyScroll(signal: ScrollSignal): ComposerCollapseEvent | null {
  if (signal.programmatic) return null;
  if (signal.direction === "up" && signal.distance > COLLAPSE_DISTANCE) {
    return { type: "scroll-away" };
  }
  if (signal.direction === "down" && signal.distance < EXPAND_DISTANCE) {
    return { type: "reach-bottom" };
  }
  return null;
}

/** Pure transition: returns the next `collapsed` flag. */
export function nextCollapsed(
  collapsed: boolean,
  event: ComposerCollapseEvent,
  ctx: ComposerCollapseContext,
): boolean {
  switch (event.type) {
    case "scroll-away":
      return ctx.hasPendingQuestion ? collapsed : true;
    case "reach-bottom":
    case "expand-click":
    case "question-appeared":
      return false;
  }
}
