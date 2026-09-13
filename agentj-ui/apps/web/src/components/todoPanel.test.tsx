import { expect, test } from "vitest";
import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import TodoPanel from "./TodoPanel";
test("renders counter, pill states and rows", () => {
  const items = [
    { id: "1", content: "解析现有 App 布局", status: "completed" as const },
    { id: "2", content: "定义三栏 grid 与宽度 token", status: "completed" as const },
    { id: "3", content: "实现 SplitPane 拖拽与持久化", status: "in_progress" as const },
    { id: "4", content: "补充响应式与键盘可达性", status: "pending" as const },
  ];
  const { container } = render(<TodoPanel items={items} />);
  expect(container.querySelector(".right-section")).not.toBeNull();
  expect(screen.getByText("2 / 4")).toBeInTheDocument();
  expect(screen.getAllByText("已完成").length).toBe(2);
  expect(screen.getByText("进行中")).toBeInTheDocument();
  expect(screen.getByText("待办")).toBeInTheDocument();
  expect(container.querySelectorAll(".aj-todo .cb.done").length).toBe(2);
  expect(container.querySelectorAll(".aj-todo .cb.prog").length).toBe(1);
});
test("empty state shows protocol gap notice", () => {
  render(<TodoPanel items={[]} />);
  expect(screen.getByText("待协议支持")).toBeInTheDocument();
});
