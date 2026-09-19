import { useEffect, useRef, useState } from "react";
import type { CSSProperties, PointerEvent as ReactPointerEvent, ReactNode } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";
import { RESIZE_HANDLES, resizeRect } from "./dialogResize";
import type { DialogRect, ResizeHandle } from "./dialogResize";

export type ModalDialogProps = {
  title: string;
  onClose: () => void;
  children: ReactNode;
  /** Optional modifier appended to `.aj-dialog-frame` (e.g. a wider settings card). */
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

type DragState = { start: DialogRect; handle: ResizeHandle; x: number; y: number };

/**
 * Shared centered modal shell. Portals to `document.body` so it escapes any
 * ancestor containing block (e.g. the topbar `backdrop-filter`). Escape and
 * mousedown on the backdrop invoke `onClose`; mousedown inside the frame is
 * ignored. The frame is resizable from its four edges and four corners.
 * Styling comes entirely from the existing `.aj-dialog*` classes.
 */
export default function ModalDialog({ title, onClose, children, cardClassName }: ModalDialogProps) {
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  const idRef = useRef<number | null>(null);
  if (idRef.current === null) idRef.current = nextModalId++;

  const frameRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<DragState | null>(null);
  const [rect, setRect] = useState<DialogRect | null>(null);
  const [resizing, setResizing] = useState(false);

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

  const startResize =
    (handle: ResizeHandle) => (event: ReactPointerEvent<HTMLSpanElement>) => {
      const frame = frameRef.current;
      if (!frame) return;
      const bounds = frame.getBoundingClientRect();
      const start: DialogRect = {
        left: bounds.left,
        top: bounds.top,
        width: bounds.width,
        height: bounds.height,
      };
      dragRef.current = { start, handle, x: event.clientX, y: event.clientY };
      setRect(start);
      setResizing(true);
      if (typeof event.currentTarget.setPointerCapture === "function") {
        event.currentTarget.setPointerCapture(event.pointerId);
      }
      event.preventDefault();
    };

  const moveResize = (event: ReactPointerEvent<HTMLSpanElement>) => {
    const drag = dragRef.current;
    if (!drag) return;
    setRect(
      resizeRect(drag.start, drag.handle, event.clientX - drag.x, event.clientY - drag.y, {
        width: window.innerWidth,
        height: window.innerHeight,
      }),
    );
  };

  const endResize = (event: ReactPointerEvent<HTMLSpanElement>) => {
    if (!dragRef.current) return;
    dragRef.current = null;
    setResizing(false);
    if (typeof event.currentTarget.releasePointerCapture === "function") {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  };

  const frameStyle: CSSProperties | undefined = rect
    ? {
        position: "fixed",
        left: rect.left,
        top: rect.top,
        width: rect.width,
        height: rect.height,
        maxWidth: "none",
        maxHeight: "none",
      }
    : undefined;

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
      <div
        ref={frameRef}
        className={`aj-dialog-frame${cardClassName ? ` ${cardClassName}` : ""}`}
        style={frameStyle}
        data-resizing={resizing ? "true" : undefined}
      >
        <div className="aj-dialog-card">
          <div className="aj-dialog__head">
            <h2 className="aj-dialog__title">{title}</h2>
            <button type="button" aria-label="close" className="aj-icon-btn" onClick={() => onCloseRef.current()}>
              <X size={16} aria-hidden="true" />
            </button>
          </div>
          {children}
        </div>
        {RESIZE_HANDLES.map((handle) => (
          <span
            key={handle}
            className="aj-dialog__resize"
            data-handle={handle}
            aria-hidden="true"
            onPointerDown={startResize(handle)}
            onPointerMove={moveResize}
            onPointerUp={endResize}
            onPointerCancel={endResize}
          />
        ))}
      </div>
    </div>,
    document.body,
  );
}
