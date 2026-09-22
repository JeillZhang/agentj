import "@testing-library/jest-dom/vitest";
import { useRef } from "react";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";
import { fireEvent, render } from "@testing-library/react";
import { useShortcutScope } from "./useShortcutScope";

function Scope({ onF, children }: { onF: () => void; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  useShortcutScope(ref, { f: onF });
  return <div ref={ref}>{children}</div>;
}

describe("useShortcutScope", () => {
  it("fires only while focus is inside the scope", () => {
    const onF = vi.fn();
    const { getByTestId } = render(
      <Scope onF={onF}>
        <button data-testid="inside">in</button>
      </Scope>,
    );
    fireEvent.keyDown(document, { key: "f" });
    expect(onF).not.toHaveBeenCalled(); // focus on body
    getByTestId("inside").focus();
    fireEvent.keyDown(document, { key: "f" });
    expect(onF).toHaveBeenCalledTimes(1);
  });

  it("ignores editable targets and modifier keys", () => {
    const onF = vi.fn();
    const { getByTestId } = render(
      <Scope onF={onF}>
        <input data-testid="q" />
      </Scope>,
    );
    getByTestId("q").focus();
    fireEvent.keyDown(document, { key: "f" });
    fireEvent.keyDown(document, { key: "f", ctrlKey: true });
    expect(onF).not.toHaveBeenCalled();
  });

  it("ignores textarea, select, and contenteditable targets", () => {
    const onF = vi.fn();
    const { getByTestId } = render(
      <Scope onF={onF}>
        <textarea data-testid="ta" />
        <select data-testid="sel" />
        <div data-testid="ce" contentEditable="true" />
      </Scope>,
    );
    for (const id of ["ta", "sel", "ce"]) {
      getByTestId(id).focus();
      fireEvent.keyDown(document, { key: "f" });
    }
    expect(onF).not.toHaveBeenCalled();
  });

  it("ignores meta, alt, and composing keystrokes", () => {
    const onF = vi.fn();
    const { getByTestId } = render(
      <Scope onF={onF}>
        <button data-testid="inside">in</button>
      </Scope>,
    );
    getByTestId("inside").focus();
    fireEvent.keyDown(document, { key: "f", metaKey: true });
    fireEvent.keyDown(document, { key: "f", altKey: true });
    fireEvent.keyDown(document, { key: "f", isComposing: true });
    expect(onF).not.toHaveBeenCalled();
  });

  it("preventDefaults only when a mapped handler actually runs", () => {
    const onF = vi.fn();
    const { getByTestId } = render(
      <Scope onF={onF}>
        <button data-testid="inside">in</button>
      </Scope>,
    );
    getByTestId("inside").focus();

    const unmapped = new KeyboardEvent("keydown", { key: "x", bubbles: true, cancelable: true });
    document.dispatchEvent(unmapped);
    expect(unmapped.defaultPrevented).toBe(false);

    const mapped = new KeyboardEvent("keydown", { key: "f", bubbles: true, cancelable: true });
    document.dispatchEvent(mapped);
    expect(mapped.defaultPrevented).toBe(true);
    expect(onF).toHaveBeenCalledTimes(1);
  });

  it("suppresses shortcuts while a modal dialog is open, even with focus in scope", () => {
    const onF = vi.fn();
    const { getByTestId } = render(
      <Scope onF={onF}>
        <button data-testid="inside">in</button>
      </Scope>,
    );
    getByTestId("inside").focus();

    const modal = document.createElement("div");
    modal.setAttribute("role", "dialog");
    modal.setAttribute("aria-modal", "true");
    document.body.appendChild(modal);

    fireEvent.keyDown(document, { key: "f" });
    expect(onF).not.toHaveBeenCalled();

    modal.remove();
    fireEvent.keyDown(document, { key: "f" });
    expect(onF).toHaveBeenCalledTimes(1);
  });

  it("does not preventDefault while a modal dialog is open", () => {
    const onF = vi.fn();
    const { getByTestId } = render(
      <Scope onF={onF}>
        <button data-testid="inside">in</button>
      </Scope>,
    );
    getByTestId("inside").focus();

    const modal = document.createElement("div");
    modal.setAttribute("role", "dialog");
    modal.setAttribute("aria-modal", "true");
    document.body.appendChild(modal);

    const event = new KeyboardEvent("keydown", { key: "f", bubbles: true, cancelable: true });
    document.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
    expect(onF).not.toHaveBeenCalled();
    modal.remove();
  });

  it("detaches the document listener on unmount", () => {
    const onF = vi.fn();
    const { getByTestId, unmount } = render(
      <Scope onF={onF}>
        <button data-testid="inside">in</button>
      </Scope>,
    );
    getByTestId("inside").focus();
    unmount();
    fireEvent.keyDown(document, { key: "f" });
    expect(onF).not.toHaveBeenCalled();
  });
});
