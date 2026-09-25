import { beforeEach, expect, test, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { render, screen, fireEvent, within } from "@testing-library/react";
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

const UUID_A = "11111111-1111-4111-8111-111111111111";
const UUID_B = "22222222-2222-4222-8222-222222222222";
const shortA = UUID_A.slice(0, 8);
const shortB = UUID_B.slice(0, 8);

const groupRow = (path: string, runId: string | null) => ({
  path,
  op: "modify" as const,
  additions: 1,
  deletions: 0,
  diff: "@@",
  toolCallId: null,
  runId,
});

test("grouped mode renders run headers and pins the current run first", () => {
  const groups = [
    { runId: UUID_A, changes: [groupRow("a.txt", UUID_A)], todos: [] },
    { runId: UUID_B, changes: [groupRow("b.txt", UUID_B)], todos: [] },
    { runId: null, changes: [groupRow("c.txt", null)], todos: [] },
  ];
  const { container } = render(
    <RightPanel collapsed={false} onToggle={() => {}} currentRunId={UUID_B} groups={groups} />,
  );
  expect(screen.getByText(`本次运行 ${shortB}`)).toBeInTheDocument();
  expect(screen.getByText(`运行 ${shortA}`)).toBeInTheDocument();
  expect(screen.getByText("未分组（历史）")).toBeInTheDocument();
  const rendered = container.querySelectorAll(".run-group");
  expect(rendered[0]).toHaveAttribute("data-run-id", UUID_B);
  expect(rendered[0]).toHaveAttribute("data-current", "true");
});

test("a group with no todos and no changes shows both empty placeholders", () => {
  const groups = [{ runId: UUID_A, changes: [], todos: [] }];
  const { container } = render(
    <RightPanel collapsed={false} onToggle={() => {}} currentRunId={null} groups={groups} />,
  );
  const group = container.querySelector(`[data-run-id="${UUID_A}"]`) as HTMLElement;
  expect(within(group).getByText("暂无任务")).toBeInTheDocument();
  expect(within(group).getByText("暂无变更")).toBeInTheDocument();
});

test("clicking a run-group head calls onSelectRun", () => {
  const onSelectRun = vi.fn();
  const groups = [{ runId: UUID_A, changes: [groupRow("a.txt", UUID_A)], todos: [] }];
  render(
    <RightPanel
      collapsed={false}
      onToggle={() => {}}
      currentRunId={UUID_A}
      groups={groups}
      onSelectRun={onSelectRun}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: `本次运行 ${shortA}` }));
  expect(onSelectRun).toHaveBeenCalledWith(UUID_A);
});

test("per-group history is filtered before reaching the ChangeList drawer", () => {
  const groups = [{ runId: UUID_A, changes: [groupRow("a.txt", UUID_A)], todos: [] }];
  const history = [
    { path: "a.txt", op: "add" as const, additions: 1, deletions: 0, diff: "@@", toolCallId: "c1", seq: 1, runId: UUID_A },
    { path: "a.txt", op: "modify" as const, additions: 2, deletions: 0, diff: "@@", toolCallId: "c1", seq: 2, runId: UUID_A },
    { path: "b.txt", op: "add" as const, additions: 3, deletions: 0, diff: "@@", toolCallId: "c2", seq: 1, runId: UUID_B },
  ];
  const { container } = render(
    <RightPanel
      collapsed={false}
      onToggle={() => {}}
      currentRunId={UUID_A}
      groups={groups}
      history={history}
    />,
  );
  expect(screen.getByText("历史记录 (2)")).toBeInTheDocument();
  expect(container.querySelectorAll(".history-entry").length).toBe(2);
});

test("css contract — run groups and history drawer are token-styled", () => {
  const css = readCss("rightpanel.css");
  for (const cls of [
    ".run-group",
    ".run-group__head",
    ".history-drawer",
    ".history-list",
    ".history-entry",
    ".hist-seq",
    ".hist-tool",
    ".hist-toolgroup",
    ".hist-counts",
    ".hist-src",
    ".trunc-hint",
    ".src-badge",
    ".src-badge.src-write",
    ".src-badge.src-bash",
  ]) {
    expect(css).toContain(cls);
  }
  expect(css).toContain('.run-group[data-current="true"] button.run-group__head');
  expect(css).toContain('.run-group[data-run-id="__legacy__"] .run-group__head');
});
