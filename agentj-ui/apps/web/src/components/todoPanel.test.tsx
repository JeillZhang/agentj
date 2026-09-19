import { beforeEach, expect, test } from "vitest";
import "@testing-library/jest-dom/vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import TodoPanel from "./TodoPanel";
import type { TodoView } from "../todoState";
beforeEach(() => localStorage.clear());
test("renders counter, pill states and rows", () => {
  const items = [
    { id: "1", content: "解析现有 App 布局", status: "completed" as const },
    { id: "2", content: "定义三栏 grid 与宽度 token", status: "completed" as const },
    { id: "3", content: "实现 SplitPane 拖拽与持久化", status: "in_progress" as const },
    { id: "4", content: "补充响应式与键盘可达性", status: "pending" as const },
  ];
  const { container } = render(<TodoPanel items={items} />);
  expect(container.querySelector(".right-section")).not.toBeNull();
  const section = container.querySelector(".right-section") as HTMLElement;
  expect(section).toHaveAttribute("data-section", "todo");
  expect(section).toHaveAttribute("data-open", "true");
  expect(screen.getByText("2 / 4")).toBeInTheDocument();
  expect(screen.getAllByText("已完成").length).toBe(2);
  expect(screen.getByText("进行中")).toBeInTheDocument();
  expect(screen.getByText("待办")).toBeInTheDocument();
  expect(container.querySelectorAll(".aj-todo .cb.done").length).toBe(2);
  expect(container.querySelectorAll(".aj-todo .cb.prog").length).toBe(1);
});
test("section collapses when the title button is clicked", () => {
  const items = [
    { id: "1", content: "解析现有 App 布局", status: "completed" as const },
    { id: "2", content: "定义三栏 grid 与宽度 token", status: "completed" as const },
    { id: "3", content: "实现 SplitPane 拖拽与持久化", status: "in_progress" as const },
    { id: "4", content: "补充响应式与键盘可达性", status: "pending" as const },
  ];
  const { container } = render(<TodoPanel items={items} />);
  const toggle = screen.getByRole("button", { name: /任务/ });
  expect(toggle).toHaveAttribute("aria-expanded", "true");
  expect(toggle).toHaveAttribute("aria-controls", "todo-section-body");
  expect(container.querySelector(".todo-list")).not.toBeNull();
  fireEvent.click(toggle);
  expect(container.querySelector(".todo-list")).toBeNull();
  expect(container.querySelector(".right-section")).not.toHaveAttribute("data-open");
  expect(screen.getByText("任务")).toBeInTheDocument();
  expect(screen.getByText("2 / 4")).toBeInTheDocument();
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  fireEvent.click(toggle);
  expect(container.querySelector(".todo-list")).not.toBeNull();
  expect(toggle).toHaveAttribute("aria-expanded", "true");
});

test("empty state shows protocol gap notice", () => {
  render(<TodoPanel items={[]} />);
  expect(screen.getByText("待协议支持")).toBeInTheDocument();
});

test("empty items always shows header with 0 / 0 counter", () => {
  render(<TodoPanel items={[]} />);
  const toggle = screen.getByRole("button", { name: /任务/ });
  expect(toggle).toHaveAttribute("aria-expanded", "true");
  expect(toggle).toHaveAttribute("aria-controls", "todo-section-body");
  expect(screen.getByText("0 / 0")).toBeInTheDocument();
  expect(screen.getByText("待协议支持")).toBeInTheDocument();
});

test("collapsed empty items hides placeholder but keeps the header", () => {
  render(<TodoPanel items={[]} />);
  const toggle = screen.getByRole("button", { name: /任务/ });
  fireEvent.click(toggle);
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  expect(screen.getByText("任务")).toBeInTheDocument();
  expect(screen.getByText("0 / 0")).toBeInTheDocument();
  expect(screen.queryByText("待协议支持")).toBeNull();
});

test("collapse state persists across unmount/remount", () => {
  const items = [
    { id: "1", content: "解析现有 App 布局", status: "completed" as const },
  ];
  const first = render(<TodoPanel items={items} />);
  fireEvent.click(screen.getByRole("button", { name: /任务/ }));
  first.unmount();
  const { container } = render(<TodoPanel items={items} />);
  const toggle = screen.getByRole("button", { name: /任务/ });
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  expect(container.querySelector(".todo-list")).toBeNull();
  fireEvent.click(toggle);
  expect(toggle).toHaveAttribute("aria-expanded", "true");
  expect(container.querySelector(".todo-list")).not.toBeNull();
});

test("renders unknown status as pending", () => {
  const items: TodoView[] = [
    { id: "1", content: "未知状态", status: "bogus" as unknown as TodoView["status"] },
  ];
  const { container } = render(<TodoPanel items={items} />);
  expect(screen.getByText("待办")).toBeInTheDocument();
  expect(container.querySelector(".aj-todo .pill-st.todo")).not.toBeNull();
});
