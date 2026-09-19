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
