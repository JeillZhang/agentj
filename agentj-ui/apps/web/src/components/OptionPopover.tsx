import { useEffect, useRef } from "react";
import type { HTMLAttributes, ReactNode, RefObject } from "react";
import "./options.css";

export type OptionPopoverProps = {
  open: boolean;
  onClose: () => void;
  align?: "left" | "right";
  width?: number;
  labelledBy?: string;
  anchorRef?: RefObject<HTMLElement | null>;
  children: ReactNode;
} & Omit<HTMLAttributes<HTMLDivElement>, "children">;

/**
 * Upward-anchored option popover primitive.
 *
 * Renders nothing while closed. While open, Escape and mousedown outside the
 * panel invoke `onClose`; mousedown inside the panel or inside `anchorRef`
 * (typically the trigger button) is ignored, so a trigger click toggles the
 * popover closed instead of close-then-reopen. Extra attributes (onKeyDown,
 * tabIndex, role) are forwarded onto the `.pop` panel so consumers can own
 * listbox keyboard behaviour.
 */
export default function OptionPopover({
  open,
  onClose,
  align,
  width,
  labelledBy,
  anchorRef,
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
      const target = event.target as Node;
      if (ref.current?.contains(target)) return;
      if (anchorRef?.current?.contains(target)) return;
      onCloseRef.current();
    };
    document.addEventListener("keydown", handleKeyDown);
    document.addEventListener("mousedown", handleMouseDown);
    ref.current?.focus?.({ preventScroll: true });
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
      document.removeEventListener("mousedown", handleMouseDown);
    };
  }, [open, anchorRef]);

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
