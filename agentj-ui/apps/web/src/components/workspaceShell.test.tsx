import { beforeEach, expect, test, vi } from "vitest";
import { render, fireEvent, screen } from "@testing-library/react";
import WorkspaceShell from "./WorkspaceShell";
beforeEach(() => localStorage.clear());
test("three columns, separators and stored width", () => {
  localStorage.setItem("agentj.layout.leftWidth", "300");
  const { container } = render(
    <WorkspaceShell right={<div>R</div>} rightCollapsed={false} onRightCollapsedChange={() => {}}>
      <div>L</div><div>C</div>
    </WorkspaceShell>);
  const root = container.querySelector(".aj-workspace") as HTMLElement;
  expect(root.style.getPropertyValue("--aj-left-w")).toBe("300px");
  expect(screen.getAllByRole("separator").length).toBe(2);
  expect(container.querySelector(".aj-shell__center")).not.toBeNull();
});
test("keyboard resize persists", () => {
  render(<WorkspaceShell right={<div>R</div>} rightCollapsed={false} onRightCollapsedChange={() => {}}>
    <div>L</div><div>C</div></WorkspaceShell>);
  fireEvent.keyDown(screen.getAllByRole("separator")[0], { key: "ArrowRight" });
  expect(localStorage.getItem("agentj.layout.leftWidth")).toBe("264");
});
test("right toggle persists collapse", () => {
  const onCollapse = vi.fn();
  render(<WorkspaceShell right={<div>R</div>} rightCollapsed={false} onRightCollapsedChange={onCollapse}>
    <div>L</div><div>C</div></WorkspaceShell>);
  fireEvent.keyDown(screen.getByRole("separator", { name: "调整右栏宽度" }), { key: "Enter" });
  expect(onCollapse).toHaveBeenCalledWith(true);
});
