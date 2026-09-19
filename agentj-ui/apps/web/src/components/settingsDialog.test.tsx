import { expect, test, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { render, screen, fireEvent, within } from "@testing-library/react";
import SettingsDialog from "./SettingsDialog";

test("defaults to the 连接 tab, uses the wide card and switches categories via the tablist", () => {
  localStorage.clear();
  render(<SettingsDialog onConnect={() => {}} onClose={() => {}} />);
  expect(document.querySelector(".aj-dialog-frame.aj-settings")).not.toBeNull();
  expect(screen.getAllByRole("tab").map((t) => t.textContent)).toEqual([
    "连接",
    "显示",
    "项目",
    "演示数据",
  ]);
  expect(screen.getByRole("tab", { name: "连接" })).toHaveAttribute("aria-selected", "true");
  expect(screen.getByLabelText("connection")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("tab", { name: "显示" }));
  expect(screen.getByRole("tab", { name: "显示" })).toHaveAttribute("aria-selected", "true");
  expect(screen.getByRole("tabpanel")).toHaveAttribute("aria-labelledby", "aj-settings-tab-display");
  expect(screen.queryByLabelText("connection")).toBeNull();

  fireEvent.click(screen.getByRole("tab", { name: "演示数据" }));
  expect(screen.getByRole("tabpanel")).toHaveAttribute("aria-labelledby", "aj-settings-tab-demo");
});

test("ArrowDown moves tab selection, focus and the tabpanel", () => {
  localStorage.clear();
  render(<SettingsDialog onConnect={() => {}} onClose={() => {}} />);
  const connection = screen.getByRole("tab", { name: "连接" });
  connection.focus();
  fireEvent.keyDown(connection, { key: "ArrowDown" });
  const display = screen.getByRole("tab", { name: "显示" });
  expect(display).toHaveAttribute("aria-selected", "true");
  expect(document.activeElement).toBe(display);
  expect(screen.getByRole("tabpanel")).toHaveAttribute("aria-labelledby", "aj-settings-tab-display");
});

test("shows the four final categories and mounts both new panels", () => {
  localStorage.clear();
  render(<SettingsDialog onConnect={() => {}} onClose={() => {}} />);
  const tabs = screen.getAllByRole("tab").map((t) => t.textContent);
  expect(tabs).toEqual(["连接", "显示", "项目", "演示数据"]);
  expect(screen.queryByText("会话列表")).toBeNull();
  fireEvent.click(screen.getByRole("tab", { name: "显示" }));
  expect(screen.getByRole("switch", { name: "减弱动画" })).toBeTruthy();
  fireEvent.click(screen.getByRole("tab", { name: "项目" }));
  expect(screen.getByLabelText("默认项目")).toBeTruthy();
});

test("keeps keyboard roaming across four tabs", () => {
  localStorage.clear();
  render(<SettingsDialog onConnect={() => {}} onClose={() => {}} />);
  const first = screen.getByRole("tab", { name: "连接" });
  first.focus();
  fireEvent.keyDown(first, { key: "ArrowDown" });
  expect(document.activeElement?.textContent).toBe("显示");
  fireEvent.keyDown(document.activeElement as Element, { key: "End" });
  expect(document.activeElement?.textContent).toBe("演示数据");
});

test("demo toggle flips the flag and reflects state text", () => {
  localStorage.clear();
  render(<SettingsDialog onConnect={() => {}} onClose={() => {}} />);
  fireEvent.click(screen.getByRole("tab", { name: "演示数据" }));
  const toggle = screen.getByRole("switch");
  expect(toggle).toHaveAttribute("aria-checked", "false");
  expect(toggle).toHaveTextContent("已关闭");
  fireEvent.click(toggle);
  expect(localStorage.getItem("agentj.demo")).toBe("1");
  expect(toggle).toHaveAttribute("aria-checked", "true");
  expect(toggle).toHaveTextContent("已开启");
});

test("connection tab submits through ConnectionBar then closes the dialog", () => {
  localStorage.clear();
  const onConnect = vi.fn();
  const onClose = vi.fn();
  render(<SettingsDialog onConnect={onConnect} onClose={onClose} />);
  const bar = screen.getByLabelText("connection");
  fireEvent.change(within(bar).getByLabelText("Host"), { target: { value: "10.0.0.5" } });
  fireEvent.change(within(bar).getByLabelText("Port"), { target: { value: "5000" } });
  fireEvent.click(within(bar).getByRole("button", { name: /connect/i }));
  expect(onConnect).toHaveBeenCalledWith("10.0.0.5", "5000", "");
  expect(onClose).toHaveBeenCalledTimes(1);
});
