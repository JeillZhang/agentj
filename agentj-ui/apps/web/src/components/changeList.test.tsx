import "@testing-library/jest-dom/vitest";
import { beforeEach, expect, test } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import ChangeList from "./ChangeList";
beforeEach(() => localStorage.clear());
const rows = [
  { path: "SplitPane.tsx", op: "add" as const, additions: 86, deletions: null, diff: null, toolCallId: null },
  { path: "App.tsx", op: "modify" as const, additions: 24, deletions: 6, diff: "+ <SplitPane>\n- <div className=\"layout\">", toolCallId: null },
];
test("rows and badges", () => {
  const { container } = render(<ChangeList changes={rows} />);
  const section = container.querySelector(".right-section") as HTMLElement;
  expect(section).toHaveAttribute("data-section", "changes");
  expect(section).toHaveAttribute("data-open", "true");
  expect(screen.getByText("+86")).toBeInTheDocument();
  expect(screen.getByText("-6")).toBeInTheDocument();
  expect(screen.getAllByText("新增").length).toBe(1);
  expect(screen.getAllByText("修改").length).toBe(1);
});
test("diff expands only when present", () => {
  const { container } = render(<ChangeList changes={rows} />);
  const buttons = container.querySelectorAll(".file-changes button");
  expect(buttons.length).toBe(1);
  fireEvent.click(buttons[0]);
  expect(container.querySelectorAll(".diff-line.add").length).toBe(1);
  expect(container.querySelectorAll(".diff-line.del").length).toBe(1);
});
test("section collapses when the title button is clicked", () => {
  const { container } = render(<ChangeList changes={rows} />);
  const toggle = screen.getByRole("button", { name: /文件变更/ });
  expect(toggle).toHaveAttribute("aria-expanded", "true");
  expect(toggle).toHaveAttribute("aria-controls", "change-section-body");
  expect(container.querySelector(".file-changes")).not.toBeNull();
  fireEvent.click(toggle);
  expect(container.querySelector(".file-changes")).toBeNull();
  expect(container.querySelector(".right-section")).not.toHaveAttribute("data-open");
  expect(screen.getByText("文件变更")).toBeInTheDocument();
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  fireEvent.click(toggle);
  expect(container.querySelector(".file-changes")).not.toBeNull();
  expect(toggle).toHaveAttribute("aria-expanded", "true");
});
test("empty state", () => {
  render(<ChangeList changes={[]} />);
  expect(screen.getByText("待协议支持")).toBeInTheDocument();
});

test("collapsed empty state hides the placeholder but keeps the header", () => {
  render(<ChangeList changes={[]} />);
  const toggle = screen.getByRole("button", { name: /文件变更/ });
  fireEvent.click(toggle);
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  expect(screen.getByText("文件变更")).toBeInTheDocument();
  expect(screen.queryByText("待协议支持")).toBeNull();
});

test("collapse state persists across unmount/remount", () => {
  const first = render(<ChangeList changes={rows} />);
  const toggle = screen.getByRole("button", { name: /文件变更/ });
  fireEvent.click(toggle);
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  first.unmount();
  const { container } = render(<ChangeList changes={rows} />);
  const again = screen.getByRole("button", { name: /文件变更/ });
  expect(again).toHaveAttribute("aria-expanded", "false");
  expect(container.querySelector(".file-changes")).toBeNull();
  fireEvent.click(again);
  expect(again).toHaveAttribute("aria-expanded", "true");
  expect(container.querySelector(".file-changes")).not.toBeNull();
});
