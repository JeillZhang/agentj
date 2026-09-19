export type ResizeHandle = "n" | "e" | "s" | "w" | "ne" | "nw" | "se" | "sw";

export type DialogRect = { left: number; top: number; width: number; height: number };

export type ViewportSize = { width: number; height: number };

export type DialogSizeLimits = { minWidth?: number; minHeight?: number };

export const RESIZE_HANDLES: readonly ResizeHandle[] = [
  "n",
  "e",
  "s",
  "w",
  "ne",
  "nw",
  "se",
  "sw",
];

export const DIALOG_MIN_WIDTH = 320;
export const DIALOG_MIN_HEIGHT = 200;
export const DIALOG_VIEWPORT_MARGIN = 8;

function clamp(n: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, n));
}

/** Per-dialog mins never exceed the viewport-capped max, so tiny screens still fit. */
function sizeBounds(viewport: ViewportSize, limits: DialogSizeLimits) {
  const maxWidth = Math.max(DIALOG_MIN_WIDTH, viewport.width - 2 * DIALOG_VIEWPORT_MARGIN);
  const maxHeight = Math.max(DIALOG_MIN_HEIGHT, viewport.height - 2 * DIALOG_VIEWPORT_MARGIN);
  return {
    minWidth: Math.min(limits.minWidth ?? DIALOG_MIN_WIDTH, maxWidth),
    minHeight: Math.min(limits.minHeight ?? DIALOG_MIN_HEIGHT, maxHeight),
    maxWidth,
    maxHeight,
  };
}

/** Applies a pointer delta to a dialog rect; opposite edges stay anchored on clamp. */
export function resizeRect(
  start: DialogRect,
  handle: ResizeHandle,
  dx: number,
  dy: number,
  viewport: ViewportSize,
  limits: DialogSizeLimits = {},
): DialogRect {
  const { minWidth, minHeight, maxWidth, maxHeight } = sizeBounds(viewport, limits);
  const anchorRight = start.left + start.width;
  const anchorBottom = start.top + start.height;
  let { left, top, width, height } = start;

  if (handle.includes("e")) width = start.width + dx;
  if (handle.includes("s")) height = start.height + dy;
  if (handle.includes("w")) {
    left = start.left + dx;
    width = start.width - dx;
  }
  if (handle.includes("n")) {
    top = start.top + dy;
    height = start.height - dy;
  }

  const capWidth = handle.includes("w")
    ? clamp(anchorRight - DIALOG_VIEWPORT_MARGIN, minWidth, maxWidth)
    : handle.includes("e")
      ? clamp(viewport.width - DIALOG_VIEWPORT_MARGIN - left, minWidth, maxWidth)
      : maxWidth;
  const capHeight = handle.includes("n")
    ? clamp(anchorBottom - DIALOG_VIEWPORT_MARGIN, minHeight, maxHeight)
    : handle.includes("s")
      ? clamp(viewport.height - DIALOG_VIEWPORT_MARGIN - top, minHeight, maxHeight)
      : maxHeight;
  width = clamp(width, minWidth, capWidth);
  height = clamp(height, minHeight, capHeight);

  if (handle.includes("w")) left = anchorRight - width;
  if (handle.includes("n")) top = anchorBottom - height;

  const maxLeft = Math.max(DIALOG_VIEWPORT_MARGIN, viewport.width - DIALOG_VIEWPORT_MARGIN - width);
  const maxTop = Math.max(DIALOG_VIEWPORT_MARGIN, viewport.height - DIALOG_VIEWPORT_MARGIN - height);
  left = clamp(left, DIALOG_VIEWPORT_MARGIN, maxLeft);
  top = clamp(top, DIALOG_VIEWPORT_MARGIN, maxTop);

  return { left, top, width, height };
}

export function clampDialogRect(
  rect: DialogRect,
  viewport: ViewportSize,
  limits: DialogSizeLimits = {},
): DialogRect {
  const { minWidth, minHeight, maxWidth, maxHeight } = sizeBounds(viewport, limits);
  const width = clamp(rect.width, minWidth, maxWidth);
  const height = clamp(rect.height, minHeight, maxHeight);
  const maxLeft = Math.max(DIALOG_VIEWPORT_MARGIN, viewport.width - DIALOG_VIEWPORT_MARGIN - width);
  const maxTop = Math.max(DIALOG_VIEWPORT_MARGIN, viewport.height - DIALOG_VIEWPORT_MARGIN - height);
  return {
    left: clamp(rect.left, DIALOG_VIEWPORT_MARGIN, maxLeft),
    top: clamp(rect.top, DIALOG_VIEWPORT_MARGIN, maxTop),
    width,
    height,
  };
}
