import { useEffect } from "react";
import type { RefObject } from "react";

/**
 * True for form fields and contenteditable hosts. Browsers report contenteditable
 * via `isContentEditable`; jsdom does not implement it, so fall back to the attribute.
 */
function isEditable(el: Element): boolean {
  if (!(el instanceof HTMLElement)) return false;
  if (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT") return true;
  if (el.isContentEditable) return true;
  const attr = el.getAttribute("contenteditable");
  return attr !== null && attr !== "false";
}

/**
 * True while a modal dialog is present in the document. ModalDialog keeps focus
 * on its opener (it only records/restores it), so focus can stay inside the
 * sidebar scope behind the modal; this guard suppresses shortcuts in that case.
 */
function hasOpenModal(): boolean {
  return document.querySelector('[role="dialog"][aria-modal="true"]') !== null;
}

/**
 * Registers document-level keydown shortcuts that only fire while focus is
 * inside `ref` (the sidebar scope) and the focused element is not editable.
 *
 * Gating (see plan Global Constraint 5):
 * - no modal dialog is open,
 * - focus must be inside the scope ref,
 * - INPUT / TEXTAREA / SELECT / contenteditable targets are ignored,
 * - ctrl / meta / alt modifiers and IME composition are ignored,
 * - preventDefault runs only when a mapped handler is invoked.
 */
export function useShortcutScope(
  ref: RefObject<HTMLElement | null>,
  handlers: Record<string, () => void>,
): void {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.ctrlKey || event.metaKey || event.altKey || event.isComposing) return;
      if (hasOpenModal()) return;
      const active = document.activeElement;
      if (!active || !ref.current?.contains(active)) return;
      if (isEditable(active)) return;
      const handler = handlers[event.key];
      if (handler) {
        event.preventDefault();
        handler();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [ref, handlers]);
}
