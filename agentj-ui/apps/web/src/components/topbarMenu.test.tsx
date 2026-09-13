import { expect, test, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { render, screen, fireEvent, within } from "@testing-library/react";
import TopbarMenu from "./TopbarMenu";

test("menu items, demo toggle and prefs events", () => {
  const onConnect = vi.fn();
  const prefs: unknown[] = [];
  window.addEventListener("agentj:sidebar-prefs", (e) => prefs.push((e as CustomEvent).detail));
  render(<TopbarMenu model="deepseek-chat" connectionLabel="已连接 ws://127.0.0.1:4096" onConnect={onConnect} onContinue={() => {}} />);
  const trigger = screen.getByRole("button", { name: "更多" });
  expect(trigger).toHaveAttribute("aria-haspopup", "menu");
  fireEvent.click(trigger);
  expect(screen.getByText(/已连接 ws:/)).toBeInTheDocument(); // status section
  expect(screen.getByText(/deepseek-chat/)).toBeInTheDocument();
  for (const label of ["连接设置…", "继续会话…", "收藏会话", "排序", "过滤", "演示数据", "设置", "帮助"]) {
    expect(screen.getByText(label)).toBeInTheDocument();
  }
  fireEvent.click(screen.getByText("演示数据"));
  expect(localStorage.getItem("agentj.demo")).toBe("1");
  fireEvent.click(screen.getByText("排序"));
  fireEvent.click(screen.getByText("创建时间"));
  expect(prefs).toContainEqual({ sort: "created" });
  localStorage.clear();
});

test("connect dialog hosts ConnectionBar and roundtrips", () => {
  const onConnect = vi.fn();
  render(<TopbarMenu model="deepseek-chat" connectionLabel="未连接" onConnect={onConnect} onContinue={() => {}} />);
  fireEvent.click(screen.getByRole("button", { name: "更多" }));
  fireEvent.click(screen.getByText("连接设置…"));
  const dialog = screen.getByLabelText("connection"); // ConnectionBar section aria-label
  fireEvent.change(within(dialog).getByLabelText("Host"), { target: { value: "127.0.0.1" } });
  fireEvent.change(within(dialog).getByLabelText("Port"), { target: { value: "4096" } });
  fireEvent.click(within(dialog).getAllByRole("button").at(-1)!); // Connect submit
  expect(onConnect).toHaveBeenCalled();
});
