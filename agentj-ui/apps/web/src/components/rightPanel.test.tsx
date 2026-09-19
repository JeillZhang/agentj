import { beforeEach, expect, test, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { render, screen, fireEvent } from "@testing-library/react";
import RightPanel from "./RightPanel";
import TodoPanel from "./TodoPanel";
import ChangeList from "./ChangeList";

// helper to read right-panel css via import.meta.dirname (mirrors a11y.test.tsx)
function readCss(name: string): string {
  return readFileSync(join(import.meta.dirname, name), "utf8");
}

const RESIZER = "调整任务与文件变更高度";

function renderBoth() {
  return render(
    <RightPanel collapsed={false} onToggle={() => {}}>
      <TodoPanel items={[]} />
      <ChangeList changes={[]} />
    </RightPanel>,
  );
}

beforeEach(() => localStorage.clear());

test("expanded header and toggle", () => {
  const onToggle = vi.fn();
  const { container } = render(<RightPanel collapsed={false} onToggle={onToggle}><div data-testid="body" /></RightPanel>);
  expect(screen.getByText("运行详情")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: /折叠右栏|展开右栏/ }));
  expect(onToggle).toHaveBeenCalledTimes(1);
  expect(container.querySelector(".aj-right")).not.toBeNull();
});

test("collapsed keeps children mounted", () => {
  const { container } = render(<RightPanel collapsed onToggle={() => {}}><div data-testid="body" /></RightPanel>);
  expect(container.classList.contains("aj-right--collapsed")).toBe(false);
  expect(container.querySelector(".aj-right")?.classList.contains("aj-right--collapsed")).toBe(true);
  expect(container.querySelector("[data-testid=body]")).not.toBeNull();
});

test("css contract — right panel fills height, anchors to the end edge and does not transition", () => {
  const css = readCss("rightpanel.css");
  const ajRight = css.match(/\.aj-right\s*\{[^}]*\}/)?.[0] ?? "";
  expect(ajRight).toMatch(/flex:\s*1/);
  expect(ajRight).toMatch(/min-height:\s*0/);
  expect(ajRight).toMatch(/align-self:\s*flex-end/);
  expect(ajRight).not.toMatch(/transition/);
  const secHead = css.match(/\.sec-head\s*\{[^}]*\}/)?.[0] ?? "";
  expect(secHead).toMatch(/font-size:\s*14px/);
});

test("resizer is absent when only one section is open", () => {
  localStorage.setItem("agentj.layout.rightSections", JSON.stringify({ todo: true, changes: false }));
  const { container } = renderBoth();
  expect(container.querySelector(".right-resizer")).toBeNull();
  expect(screen.queryByRole("separator")).toBeNull();
});

test("resizer appears with the aria contract when both sections are open", () => {
  const { container } = renderBoth();
  const sep = screen.getByRole("separator", { name: RESIZER });
  expect(sep).toHaveAttribute("aria-orientation", "horizontal");
  expect(sep).toHaveAttribute("aria-valuemin", "15");
  expect(sep).toHaveAttribute("aria-valuemax", "85");
  expect(sep).toHaveAttribute("aria-valuenow", "50");
  expect(container.querySelector(".aj-right__body")).toHaveAttribute("data-both-open", "true");
});

test("ArrowUp/ArrowDown adjust and persist the split; Shift steps further", () => {
  localStorage.setItem("agentj.layout.rightSectionSplit", "0.5");
  renderBoth();
  const sep = screen.getByRole("separator", { name: RESIZER });
  expect(sep).toHaveAttribute("aria-valuenow", "50");
  fireEvent.keyDown(sep, { key: "ArrowUp" });
  expect(Number(localStorage.getItem("agentj.layout.rightSectionSplit"))).toBeCloseTo(0.48);
  expect(screen.getByRole("separator", { name: RESIZER })).toHaveAttribute("aria-valuenow", "48");
  fireEvent.keyDown(screen.getByRole("separator", { name: RESIZER }), { key: "ArrowDown", shiftKey: true });
  expect(Number(localStorage.getItem("agentj.layout.rightSectionSplit"))).toBeCloseTo(0.56);
  expect(screen.getByRole("separator", { name: RESIZER })).toHaveAttribute("aria-valuenow", "56");
});

test("Home and double-click reset the split to auto", () => {
  localStorage.setItem("agentj.layout.rightSectionSplit", "0.3");
  renderBoth();
  const body = document.querySelector(".aj-right__body");
  expect(body).toHaveAttribute("data-split", "custom");
  fireEvent.keyDown(screen.getByRole("separator", { name: RESIZER }), { key: "Home" });
  expect(localStorage.getItem("agentj.layout.rightSectionSplit")).toBeNull();
  expect(document.querySelector(".aj-right__body")).not.toHaveAttribute("data-split");

  localStorage.setItem("agentj.layout.rightSectionSplit", "0.3");
  renderBoth();
  fireEvent.doubleClick(screen.getAllByRole("separator", { name: RESIZER }).at(-1)!);
  expect(localStorage.getItem("agentj.layout.rightSectionSplit")).toBeNull();
});

test("pointer drag persists the inverse-mapped ratio", () => {
  const { container } = renderBoth();
  const body = container.querySelector(".aj-right__body") as HTMLElement;
  body.getBoundingClientRect = () =>
    ({ top: 100, height: 400, bottom: 500, left: 0, right: 400, width: 400, x: 0, y: 100, toJSON: () => ({}) }) as DOMRect;
  const sep = screen.getByRole("separator", { name: RESIZER }) as HTMLElement;
  (sep as HTMLElement & { setPointerCapture?: unknown }).setPointerCapture = () => {};
  fireEvent.pointerDown(sep, { clientY: 100, pointerId: 1 });
  fireEvent.pointerMove(sep, { clientY: 403, pointerId: 1 });
  fireEvent.pointerUp(sep, { pointerId: 1 });
  expect(Number(localStorage.getItem("agentj.layout.rightSectionSplit"))).toBeCloseTo(0.75);
});

test("a zero-height body drag is a no-op and does not throw", () => {
  renderBoth();
  const sep = screen.getByRole("separator", { name: RESIZER });
  expect(() => {
    fireEvent.pointerDown(sep, { clientY: 100, pointerId: 1 });
    fireEvent.pointerMove(sep, { clientY: 200, pointerId: 1 });
    fireEvent.pointerUp(sep, { pointerId: 1 });
  }).not.toThrow();
  expect(localStorage.getItem("agentj.layout.rightSectionSplit")).toBeNull();
});

test("a stored split is restored across unmount/remount", () => {
  localStorage.setItem("agentj.layout.rightSectionSplit", "0.3");
  const first = renderBoth();
  expect(first.container.querySelector(".aj-right__body")).toHaveAttribute("data-split", "custom");
  expect(screen.getByRole("separator", { name: RESIZER })).toHaveAttribute("aria-valuenow", "30");
  first.unmount();
  const second = renderBoth();
  expect(second.container.querySelector(".aj-right__body")).toHaveAttribute("data-split", "custom");
  expect(screen.getByRole("separator", { name: RESIZER })).toHaveAttribute("aria-valuenow", "30");
});
