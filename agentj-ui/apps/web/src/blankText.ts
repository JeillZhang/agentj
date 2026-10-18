// Text that renders as nothing at all.
//
// Gateways keep the stream warm between tool calls by sending a lone
// zero-width space (U+200B). It is NOT Unicode White_Space, so neither
// `String.prototype.trim()` nor `/^\s*$/` removes it — it has to be listed.
//
// Anything that survives this strip is real content.

export const BLANK_CHARS = /[\s\u200b-\u200f\u2060\ufeff]/g;

/** True when `text` carries no visible glyph (empty, whitespace-only, zero-width-only). */
export function isBlankText(text: string): boolean {
  return text.replace(BLANK_CHARS, "") === "";
}
