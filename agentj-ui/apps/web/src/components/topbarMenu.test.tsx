import { expect, test, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { render, screen, fireEvent, within } from "@testing-library/react";
import TopbarMenu from "./TopbarMenu";
import { createProject, setCurrentProject } from "../projectStore";

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
  for (const label of ["连接设置…", "项目", "继续会话…", "收藏会话", "排序", "过滤", "演示数据", "设置", "帮助"]) {
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

test("connect dialog is portaled to document.body", () => {
  render(<TopbarMenu connectionLabel="未连接" onConnect={() => {}} onContinue={() => {}} />);
  fireEvent.click(screen.getByRole("button", { name: "更多" }));
  fireEvent.click(screen.getByText("连接设置…"));
  const dialog = screen.getByLabelText("连接设置");
  // Portal escapes the topbar containing block (backdrop-filter creates one).
  expect(dialog.parentElement).toBe(document.body);
});

test("connect dialog closes on backdrop mousedown but stays open on card mousedown", () => {
  render(<TopbarMenu connectionLabel="未连接" onConnect={() => {}} onContinue={() => {}} />);

  fireEvent.click(screen.getByRole("button", { name: "更多" }));
  fireEvent.click(screen.getByText("连接设置…"));
  fireEvent.mouseDown(screen.getByLabelText("连接设置"));
  expect(screen.queryByLabelText("连接设置")).not.toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "更多" }));
  fireEvent.click(screen.getByText("连接设置…"));
  const reopened = screen.getByLabelText("连接设置");
  fireEvent.mouseDown(reopened.querySelector(".aj-dialog-card")!);
  expect(screen.getByLabelText("连接设置")).toBeInTheDocument();
});

test("continue dialog is portaled to document.body", () => {
  render(<TopbarMenu connectionLabel="未连接" onConnect={() => {}} onContinue={() => {}} />);
  fireEvent.click(screen.getByRole("button", { name: "更多" }));
  fireEvent.click(screen.getByText("继续会话…"));
  const dialog = screen.getByLabelText("继续会话");
  expect(dialog.parentElement).toBe(document.body);
});

test("项目 sits directly after 连接设置… in the menu", () => {
  localStorage.clear();
  render(<TopbarMenu connectionLabel="未连接" onConnect={() => {}} onContinue={() => {}} />);
  fireEvent.click(screen.getByRole("button", { name: "更多" }));
  const labels = screen.getAllByRole("menuitem").map((el) => el.textContent ?? "");
  const idx = labels.findIndex((text) => text.includes("连接设置"));
  expect(idx).toBeGreaterThanOrEqual(0);
  expect(labels[idx + 1]).toBe("项目");
  localStorage.clear();
});

test("项目 submenu lists projects and marks the current one", () => {
  localStorage.clear();
  const alpha = createProject("Alpha");
  setCurrentProject(alpha.id);
  render(<TopbarMenu connectionLabel="未连接" onConnect={() => {}} onContinue={() => {}} />);
  fireEvent.click(screen.getByRole("button", { name: "更多" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "项目" }));
  const group = screen.getByRole("group", { name: "项目" });
  expect(within(group).getByRole("menuitemradio", { name: /Inbox/ })).toHaveAttribute("aria-checked", "false");
  const current = within(group).getByRole("menuitemradio", { name: /Alpha/ });
  expect(current).toHaveAttribute("aria-checked", "true");
  expect(current).toHaveTextContent("✓");
  localStorage.clear();
});

test("clicking a project persists it and closes the menu", () => {
  localStorage.clear();
  const beta = createProject("Beta");
  render(<TopbarMenu connectionLabel="未连接" onConnect={() => {}} onContinue={() => {}} />);
  fireEvent.click(screen.getByRole("button", { name: "更多" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "项目" }));
  fireEvent.click(within(screen.getByRole("group", { name: "项目" })).getByRole("menuitemradio", { name: /Beta/ }));
  expect(localStorage.getItem("agentj.currentProject")).toBe(beta.id);
  expect(screen.queryByRole("menu")).toBeNull();
  localStorage.clear();
});

test("＋ 添加项目 opens the dialog and creates + selects the project", () => {
  localStorage.clear();
  render(<TopbarMenu connectionLabel="未连接" onConnect={() => {}} onContinue={() => {}} />);
  fireEvent.click(screen.getByRole("button", { name: "更多" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "项目" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "＋ 添加项目" }));
  const dialog = screen.getByLabelText("添加项目");
  const addButton = within(dialog).getByRole("button", { name: "添加" });
  expect(addButton).toBeDisabled();
  fireEvent.change(within(dialog).getByLabelText("项目名称"), { target: { value: "Gamma" } });
  fireEvent.click(addButton);
  expect(screen.queryByLabelText("添加项目")).not.toBeInTheDocument();
  const projects = JSON.parse(localStorage.getItem("agentj.projects") ?? "[]") as Array<{ id: string; name: string }>;
  const gamma = projects.find((p) => p.name === "Gamma");
  expect(gamma).toBeTruthy();
  expect(localStorage.getItem("agentj.currentProject")).toBe(gamma?.id);
  localStorage.clear();
});
