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

/** Applies a pointer delta to a dialog rect; opposite edges stay anchored on clamp. */
export function resizeRect(
  start: DialogRect,
  handle: ResizeHandle,
  dx: number,
  dy: number,
  viewport: ViewportSize,
  limits: DialogSizeLimits = {},
): DialogRect {
  const minWidth = limits.minWidth ?? DIALOG_MIN_WIDTH;
  const minHeight = limits.minHeight ?? DIALOG_MIN_HEIGHT;
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

  if (width < minWidth) {
    width = minWidth;
    if (handle.includes("w")) left = anchorRight - width;
  }
  if (height < minHeight) {
    height = minHeight;
    if (handle.includes("n")) top = anchorBottom - height;
  }

  const maxWidth = Math.max(minWidth, viewport.width - 2 * DIALOG_VIEWPORT_MARGIN);
  const maxHeight = Math.max(minHeight, viewport.height - 2 * DIALOG_VIEWPORT_MARGIN);
  if (width > maxWidth) {
    width = maxWidth;
    if (handle.includes("w")) left = anchorRight - width;
  }
  if (height > maxHeight) {
    height = maxHeight;
    if (handle.includes("n")) top = anchorBottom - height;
  }

  const maxLeft = Math.max(DIALOG_VIEWPORT_MARGIN, viewport.width - DIALOG_VIEWPORT_MARGIN - width);
  const maxTop = Math.max(DIALOG_VIEWPORT_MARGIN, viewport.height - DIALOG_VIEWPORT_MARGIN - height);
  left = clamp(left, DIALOG_VIEWPORT_MARGIN, maxLeft);
  top = clamp(top, DIALOG_VIEWPORT_MARGIN, maxTop);

  return { left, top, width, height };
}
