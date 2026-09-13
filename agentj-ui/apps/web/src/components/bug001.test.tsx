import "@testing-library/jest-dom/vitest";
import { beforeEach, expect, test, vi } from "vitest";
import { render, fireEvent, screen } from "@testing-library/react";
import WorkspaceShell from "./WorkspaceShell";

// BUG-001 regression guards. When the right column is collapsed the CSS
// (layout.css .aj-workspace--right-collapsed) pins the wrapper to a 40px rail,
// but the right separator used to stay at the stored width (aria-valuenow=320)
// and kept accepting drag/arrow resize that persisted a width inconsistent with
// the 40px visual. While collapsed the splitter must be locked (40/40/40, resize
// a no-op, nothing written to localStorage) and expanding must restore the
// stored width.

const RIGHT_SEP = () => screen.getByRole("separator", { name: "调整右栏宽度" });

function renderShell(collapsed: boolean, onCollapsedChange = () => {}) {
  return render(
    <WorkspaceShell right={<div>R</div>} rightCollapsed={collapsed} onRightCollapsedChange={onCollapsedChange}>
      <div>L</div>
      <div>C</div>
    </WorkspaceShell>,
  );
}

beforeEach(() => localStorage.clear());

test("collapsed right separator is locked to the 40px rail (aria + min/max)", () => {
  renderShell(true);
  expect(RIGHT_SEP()).toHaveAttribute("aria-valuenow", "40");
  expect(RIGHT_SEP()).toHaveAttribute("aria-valuemin", "40");
  expect(RIGHT_SEP()).toHaveAttribute("aria-valuemax", "40");
});

test("collapsed keyboard resize does not persist a width", () => {
  localStorage.setItem("agentj.layout.rightWidth", "360");
  renderShell(true);
  fireEvent.keyDown(RIGHT_SEP(), { key: "ArrowLeft" });
  expect(localStorage.getItem("agentj.layout.rightWidth")).toBe("360");
  expect(RIGHT_SEP()).toHaveAttribute("aria-valuenow", "40");
});

test("collapsed drag does not persist a width", () => {
  localStorage.setItem("agentj.layout.rightWidth", "360");
  renderShell(true);
  const sep = RIGHT_SEP() as HTMLElement;
  (sep as HTMLElement & { setPointerCapture?: unknown }).setPointerCapture = () => {};
  fireEvent.pointerDown(sep, { clientX: 100, pointerId: 1 });
  fireEvent.pointerMove(sep, { clientX: 130, pointerId: 1 });
  fireEvent.pointerUp(sep, { pointerId: 1 });
  expect(localStorage.getItem("agentj.layout.rightWidth")).toBe("360");
});

test("Enter still expands a collapsed right column", () => {
  const onCollapsedChange = vi.fn();
  renderShell(true, onCollapsedChange);
  fireEvent.keyDown(RIGHT_SEP(), { key: "Enter" });
  expect(onCollapsedChange).toHaveBeenCalledWith(false);
});

test("expanding restores the stored right width", () => {
  localStorage.setItem("agentj.layout.rightWidth", "360");
  const { rerender } = renderShell(true);
  expect(RIGHT_SEP()).toHaveAttribute("aria-valuenow", "40");
  rerender(
    <WorkspaceShell right={<div>R</div>} rightCollapsed={false} onRightCollapsedChange={() => {}}>
      <div>L</div>
      <div>C</div>
    </WorkspaceShell>,
  );
  expect(RIGHT_SEP()).toHaveAttribute("aria-valuenow", "360");
});