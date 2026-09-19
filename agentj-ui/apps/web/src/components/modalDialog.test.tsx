import { expect, test, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import ModalDialog from "./ModalDialog";

test("renders a centered dialog portaled to document.body", () => {
  render(
    <ModalDialog title="添加项目" onClose={() => {}}>
      <p>body</p>
    </ModalDialog>,
  );
  const dialog = screen.getByLabelText("添加项目");
  expect(dialog.parentElement).toBe(document.body);
  expect(dialog).toHaveAttribute("aria-modal", "true");
  expect(dialog.querySelector(".aj-dialog-card")).toBeInTheDocument();
});

test("Escape invokes onClose", () => {
  const onClose = vi.fn();
  render(
    <ModalDialog title="添加项目" onClose={onClose}>
      <p>body</p>
    </ModalDialog>,
  );
  fireEvent.keyDown(document, { key: "Escape" });
  expect(onClose).toHaveBeenCalledTimes(1);
});

test("backdrop mousedown closes but card mousedown does not", () => {
  const onClose = vi.fn();
  render(
    <ModalDialog title="添加项目" onClose={onClose}>
      <p>body</p>
    </ModalDialog>,
  );
  const dialog = screen.getByLabelText("添加项目");
  fireEvent.mouseDown(dialog.querySelector(".aj-dialog-card")!);
  expect(onClose).not.toHaveBeenCalled();
  fireEvent.mouseDown(dialog);
  expect(onClose).toHaveBeenCalledTimes(1);
});

test("close button invokes onClose", () => {
  const onClose = vi.fn();
  render(
    <ModalDialog title="添加项目" onClose={onClose}>
      <p>body</p>
    </ModalDialog>,
  );
  fireEvent.click(screen.getByLabelText("close"));
  expect(onClose).toHaveBeenCalledTimes(1);
});

test("closes only the top-most dialog on Escape", () => {
  const closeA = vi.fn();
  const closeB = vi.fn();
  render(<ModalDialog title="A" onClose={closeA}>a</ModalDialog>);
  render(<ModalDialog title="B" onClose={closeB}>b</ModalDialog>);
  fireEvent.keyDown(document, { key: "Escape" });
  expect(closeB).toHaveBeenCalledTimes(1);
  expect(closeA).not.toHaveBeenCalled();
});

test("passes Escape through to the next dialog after the top one unmounts", () => {
  const closeA = vi.fn();
  const closeB = vi.fn();
  const { rerender } = render(
    <>
      <ModalDialog title="A" onClose={closeA}>a</ModalDialog>
      <ModalDialog title="B" onClose={closeB}>b</ModalDialog>
    </>,
  );
  fireEvent.keyDown(document, { key: "Escape" });
  rerender(<ModalDialog title="A" onClose={closeA}>a</ModalDialog>);
  fireEvent.keyDown(document, { key: "Escape" });
  expect(closeA).toHaveBeenCalledTimes(1);
});

test("backdrop mousedown closes only the top-most dialog", () => {
  const closeA = vi.fn();
  const closeB = vi.fn();
  render(
    <>
      <ModalDialog title="A" onClose={closeA}>a</ModalDialog>
      <ModalDialog title="B" onClose={closeB}>b</ModalDialog>
    </>,
  );
  fireEvent.mouseDown(screen.getByLabelText("B"));
  expect(closeB).toHaveBeenCalledTimes(1);
  expect(closeA).not.toHaveBeenCalled();
  expect(screen.getByLabelText("A")).toBeInTheDocument();
});

function stubRect(frame: HTMLElement, rect: { left: number; top: number; width: number; height: number }) {
  frame.getBoundingClientRect = () =>
    ({
      ...rect,
      right: rect.left + rect.width,
      bottom: rect.top + rect.height,
      x: rect.left,
      y: rect.top,
      toJSON: () => ({}),
    }) as DOMRect;
}

function seHandle(dialog: HTMLElement): HTMLElement {
  const handle = dialog.querySelector('[data-handle="se"]') as HTMLElement;
  (handle as HTMLElement & { setPointerCapture?: unknown }).setPointerCapture = () => {};
  return handle;
}

test("renders eight resize handles on the dialog frame", () => {
  render(<ModalDialog title="A" onClose={() => {}}><p /></ModalDialog>);
  const dialog = screen.getByLabelText("A");
  expect(dialog.querySelector(".aj-dialog-frame")).not.toBeNull();
  const handles = [...dialog.querySelectorAll<HTMLElement>(".aj-dialog__resize")];
  expect(handles.map((h) => h.dataset.handle).sort()).toEqual([
    "e", "n", "ne", "nw", "s", "se", "sw", "w",
  ]);
});

test("dragging the se handle resizes and fixes the frame position", () => {
  render(<ModalDialog title="A" onClose={() => {}}><p /></ModalDialog>);
  const dialog = screen.getByLabelText("A");
  const frame = dialog.querySelector(".aj-dialog-frame") as HTMLElement;
  stubRect(frame, { left: 100, top: 100, width: 560, height: 400 });
  const se = seHandle(dialog);
  fireEvent.pointerDown(se, { clientX: 660, clientY: 500, pointerId: 1 });
  fireEvent.pointerMove(se, { clientX: 760, clientY: 560, pointerId: 1 });
  expect(frame.style.position).toBe("fixed");
  expect(frame.style.left).toBe("100px");
  expect(frame.style.top).toBe("100px");
  expect(frame.style.width).toBe("660px");
  expect(frame.style.height).toBe("460px");
  fireEvent.pointerUp(se, { pointerId: 1 });
  expect(frame.dataset.resizing).toBeUndefined();
});

test("dragging the nw handle anchors the opposite corner", () => {
  render(<ModalDialog title="A" onClose={() => {}}><p /></ModalDialog>);
  const dialog = screen.getByLabelText("A");
  const frame = dialog.querySelector(".aj-dialog-frame") as HTMLElement;
  stubRect(frame, { left: 100, top: 100, width: 560, height: 400 });
  const nw = dialog.querySelector('[data-handle="nw"]') as HTMLElement;
  (nw as HTMLElement & { setPointerCapture?: unknown }).setPointerCapture = () => {};
  fireEvent.pointerDown(nw, { clientX: 100, clientY: 100, pointerId: 2 });
  fireEvent.pointerMove(nw, { clientX: 60, clientY: 70, pointerId: 2 });
  expect(frame.style.left).toBe("60px");
  expect(frame.style.top).toBe("70px");
  expect(frame.style.width).toBe("600px");
  expect(frame.style.height).toBe("430px");
  fireEvent.pointerUp(nw, { pointerId: 2 });
});

test("resizing cannot shrink a dialog below the minimum size", () => {
  render(<ModalDialog title="A" onClose={() => {}}><p /></ModalDialog>);
  const dialog = screen.getByLabelText("A");
  const frame = dialog.querySelector(".aj-dialog-frame") as HTMLElement;
  stubRect(frame, { left: 100, top: 100, width: 560, height: 400 });
  const se = seHandle(dialog);
  fireEvent.pointerDown(se, { clientX: 660, clientY: 500, pointerId: 1 });
  fireEvent.pointerMove(se, { clientX: -4000, clientY: -4000, pointerId: 1 });
  expect(frame.style.width).toBe("320px");
  expect(frame.style.height).toBe("200px");
  fireEvent.pointerUp(se, { pointerId: 1 });
});

test("honors per-dialog minimum size overrides", () => {
  render(
    <ModalDialog title="A" onClose={() => {}} minWidth={480} minHeight={300}>
      <p />
    </ModalDialog>,
  );
  const dialog = screen.getByLabelText("A");
  const frame = dialog.querySelector(".aj-dialog-frame") as HTMLElement;
  stubRect(frame, { left: 100, top: 100, width: 560, height: 400 });
  const se = seHandle(dialog);
  fireEvent.pointerDown(se, { clientX: 660, clientY: 500, pointerId: 1 });
  fireEvent.pointerMove(se, { clientX: -4000, clientY: -4000, pointerId: 1 });
  expect(frame.style.width).toBe("480px");
  expect(frame.style.height).toBe("300px");
  fireEvent.pointerUp(se, { pointerId: 1 });
});

test("ignores non-primary pointer buttons", () => {
  render(<ModalDialog title="A" onClose={() => {}}><p /></ModalDialog>);
  const dialog = screen.getByLabelText("A");
  const frame = dialog.querySelector(".aj-dialog-frame") as HTMLElement;
  stubRect(frame, { left: 100, top: 100, width: 560, height: 400 });
  const se = seHandle(dialog);
  fireEvent.pointerDown(se, { clientX: 660, clientY: 500, pointerId: 1, button: 2 });
  fireEvent.pointerMove(se, { clientX: 760, clientY: 560, pointerId: 1 });
  expect(frame.style.position).toBe("");
  expect(frame.style.width).toBe("");
});

test("pointercancel ends the resize without leaving drag state behind", () => {
  render(<ModalDialog title="A" onClose={() => {}}><p /></ModalDialog>);
  const dialog = screen.getByLabelText("A");
  const frame = dialog.querySelector(".aj-dialog-frame") as HTMLElement;
  stubRect(frame, { left: 100, top: 100, width: 560, height: 400 });
  const se = seHandle(dialog);
  fireEvent.pointerDown(se, { clientX: 660, clientY: 500, pointerId: 1 });
  fireEvent.pointerMove(se, { clientX: 700, clientY: 530, pointerId: 1 });
  expect(frame.dataset.resizing).toBe("true");
  fireEvent.pointerCancel(se, { pointerId: 1 });
  expect(frame.dataset.resizing).toBeUndefined();
});

test("re-clamps a manually sized dialog when the viewport shrinks", () => {
  render(<ModalDialog title="A" onClose={() => {}}><p /></ModalDialog>);
  const dialog = screen.getByLabelText("A");
  const frame = dialog.querySelector(".aj-dialog-frame") as HTMLElement;
  stubRect(frame, { left: 100, top: 100, width: 560, height: 400 });
  const se = seHandle(dialog);
  fireEvent.pointerDown(se, { clientX: 660, clientY: 500, pointerId: 1 });
  fireEvent.pointerMove(se, { clientX: 760, clientY: 560, pointerId: 1 });
  fireEvent.pointerUp(se, { pointerId: 1 });
  expect(frame.style.width).toBe("660px");
  const innerWidth = window.innerWidth;
  Object.defineProperty(window, "innerWidth", { configurable: true, value: 500 });
  try {
    fireEvent(window, new Event("resize"));
    expect(frame.style.width).toBe("484px");
    expect(frame.style.left).toBe("8px");
  } finally {
    Object.defineProperty(window, "innerWidth", { configurable: true, value: innerWidth });
  }
});
