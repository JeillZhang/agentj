import "@testing-library/jest-dom/vitest";
import { expect, test } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import ChangeList from "./ChangeList";
const rows = [
  { path: "SplitPane.tsx", op: "add" as const, additions: 86, deletions: null, diff: null, toolCallId: null },
  { path: "App.tsx", op: "modify" as const, additions: 24, deletions: 6, diff: "+ <SplitPane>\n- <div className=\"layout\">", toolCallId: null },
];
test("rows and badges", () => {
  render(<ChangeList changes={rows} />);
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
