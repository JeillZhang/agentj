import { useEffect, useRef } from "react";
import type { HTMLAttributes, ReactNode } from "react";
import "./options.css";

export type OptionPopoverProps = {
  open: boolean;
  onClose: () => void;
  align?: "left" | "right";
  width?: number;
  labelledBy?: string;
  children: ReactNode;
} & Omit<HTMLAttributes<HTMLDivElement>, "children">;

/**
 * Upward-anchored option popover primitive.
 *
 * Renders nothing while closed. While open, Escape and mousedown outside the
 * panel invoke `onClose`; mousedown inside the panel is ignored. Extra
 * attributes (onKeyDown, tabIndex, role) are forwarded onto the `.pop` panel so
 * consumers can own listbox keyboard behaviour.
 */
export default function OptionPopover({
  open,
  onClose,
  align,
  width,
  labelledBy,
  children,
  ...rest
}: OptionPopoverProps) {
  const ref = useRef<HTMLDivElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  useEffect(() => {
    if (!open) return;
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onCloseRef.current();
    };
    const handleMouseDown = (event: MouseEvent) => {
      if (!ref.current?.contains(event.target as Node)) onCloseRef.current();
    };
    document.addEventListener("keydown", handleKeyDown);
    document.addEventListener("mousedown", handleMouseDown);
    ref.current?.focus?.({ preventScroll: true });
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
      document.removeEventListener("mousedown", handleMouseDown);
    };
  }, [open]);

  if (!open) return null;

  return (
    <div
      ref={ref}
      className={`pop ${align === "right" ? "pop-right" : "pop-left"}`}
      style={width ? { width } : undefined}
      role="group"
      aria-labelledby={labelledBy}
      {...rest}
    >
      {children}
    </div>
  );
}
