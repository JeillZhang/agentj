import { useEffect, useRef } from "react";
import type { ReactNode } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";

export type ModalDialogProps = {
  title: string;
  onClose: () => void;
  children: ReactNode;
  /** Optional modifier appended to `.aj-dialog-card` (e.g. a wider settings card). */
  cardClassName?: string;
};

// Module-level stack of mounted dialogs (bottom → top). Only the top-most
// instance reacts to Escape/backdrop dismissal so nested dialogs (settings →
// project edit) close one layer at a time.
const modalStack: number[] = [];
let nextModalId = 1;

function isTopModal(id: number): boolean {
  return modalStack[modalStack.length - 1] === id;
}

/**
 * Shared centered modal shell. Portals to `document.body` so it escapes any
 * ancestor containing block (e.g. the topbar `backdrop-filter`). Escape and
 * mousedown on the backdrop invoke `onClose`; mousedown inside the card is
 * ignored. Styling comes entirely from the existing `.aj-dialog*` classes.
 */
export default function ModalDialog({ title, onClose, children, cardClassName }: ModalDialogProps) {
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  const idRef = useRef<number | null>(null);
  if (idRef.current === null) idRef.current = nextModalId++;

  useEffect(() => {
    const id = idRef.current;
    if (id === null) return;
    modalStack.push(id);
    return () => {
      const index = modalStack.indexOf(id);
      if (index !== -1) modalStack.splice(index, 1);
    };
  }, []);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      const id = idRef.current;
      if (event.key === "Escape" && id !== null && isTopModal(id)) onCloseRef.current();
    };
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, []);

  return createPortal(
    <div
      role="dialog"
      aria-label={title}
      aria-modal="true"
      className="aj-dialog"
      onMouseDown={(e) => {
        const id = idRef.current;
        if (e.target === e.currentTarget && id !== null && isTopModal(id)) onCloseRef.current();
      }}
    >
      <div className={`aj-dialog-card${cardClassName ? ` ${cardClassName}` : ""}`}>
        <div className="aj-dialog__head">
          <h2 className="aj-dialog__title">{title}</h2>
          <button type="button" aria-label="close" className="aj-icon-btn" onClick={() => onCloseRef.current()}>
            <X size={16} aria-hidden="true" />
          </button>
        </div>
        {children}
      </div>
    </div>,
    document.body,
  );
}
