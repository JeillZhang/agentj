import "@testing-library/jest-dom/vitest";
import { expect, test, vi } from "vitest";
import { render, fireEvent } from "@testing-library/react";
import SplitPane from "./SplitPane";
test("keyboard resize, reset and aria", () => {
  const onResize = vi.fn(); const onReset = vi.fn();
  const { getByRole } = render(
    <SplitPane side="left" width={256} min={200} max={420} defaultWidth={256}
      onResize={onResize} onReset={onReset} />,
  );
  const sep = getByRole("separator");
  expect(sep).toHaveAttribute("aria-valuenow", "256");
  expect(sep).toHaveAttribute("aria-valuemin", "200");
  fireEvent.keyDown(sep, { key: "ArrowRight" });
  expect(onResize).toHaveBeenLastCalledWith(264);
  fireEvent.keyDown(sep, { key: "ArrowRight", shiftKey: true });
  expect(onResize).toHaveBeenLastCalledWith(288);
  fireEvent.keyDown(sep, { key: "Home" });
  expect(onReset).toHaveBeenCalled();
  fireEvent.doubleClick(sep);
  expect(onReset).toHaveBeenCalledTimes(2);
});
test("pointer drag clamps to max", () => {
  const onResize = vi.fn();
  const { getByRole } = render(
    <SplitPane side="left" width={256} min={200} max={420} defaultWidth={256}
      onResize={onResize} onReset={() => {}} />,
  );
  const sep = getByRole("separator") as HTMLElement;
  (sep as HTMLElement & { setPointerCapture?: unknown }).setPointerCapture = () => {};
  fireEvent.pointerDown(sep, { clientX: 100, pointerId: 1 });
  fireEvent.pointerMove(sep, { clientX: 130, pointerId: 1 });
  expect(onResize).toHaveBeenLastCalledWith(286);
  fireEvent.pointerMove(sep, { clientX: 900, pointerId: 1 });
  expect(onResize).toHaveBeenLastCalledWith(420);
  fireEvent.pointerUp(sep, { pointerId: 1 });
});
test("commit on pointerup and restore on pointercancel", () => {
  const onResize = vi.fn(); const onCommit = vi.fn();
  const { getByRole } = render(
    <SplitPane side="left" width={256} min={200} max={420} defaultWidth={256}
      onResize={onResize} onCommit={onCommit} onReset={() => {}} />,
  );
  const sep = getByRole("separator") as HTMLElement;
  (sep as HTMLElement & { setPointerCapture?: unknown }).setPointerCapture = () => {};
  fireEvent.pointerDown(sep, { clientX: 100, pointerId: 1 });
  fireEvent.pointerMove(sep, { clientX: 130, pointerId: 1 });
  fireEvent.pointerUp(sep, { pointerId: 1 });
  expect(onCommit).toHaveBeenCalledTimes(1);
  expect(onCommit).toHaveBeenLastCalledWith(286);
  fireEvent.pointerDown(sep, { clientX: 200, pointerId: 2 });
  fireEvent.pointerMove(sep, { clientX: 260, pointerId: 2 });
  fireEvent.pointerCancel(sep, { pointerId: 2 });
  expect(onResize).toHaveBeenLastCalledWith(256);
  expect(onCommit).toHaveBeenCalledTimes(1);
});
test("right side Enter toggles collapse and arrows reverse", () => {
  const onResize = vi.fn(); const onToggleCollapse = vi.fn();
  const { getByRole } = render(
    <SplitPane side="right" width={320} min={260} max={480} defaultWidth={320}
      onResize={onResize} onReset={() => {}} onToggleCollapse={onToggleCollapse} />,
  );
  const sep = getByRole("separator");
  fireEvent.keyDown(sep, { key: "ArrowLeft" });
  expect(onResize).toHaveBeenLastCalledWith(328);
  fireEvent.keyDown(sep, { key: "Enter" });
  expect(onToggleCollapse).toHaveBeenCalled();
});
