import { expect, test, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { render, screen, fireEvent, within } from "@testing-library/react";
import SettingsDialog from "./SettingsDialog";
import { SIDEBAR_PREFS_KEY, loadSidebarPrefs } from "../sidebarPrefsStore";

test("defaults to the 连接 tab, uses the wide card and switches categories via the tablist", () => {
  localStorage.clear();
  render(<SettingsDialog onConnect={() => {}} onClose={() => {}} />);
  expect(document.querySelector(".aj-dialog-card.aj-settings")).not.toBeNull();
  expect(screen.getAllByRole("tab").map((t) => t.textContent)).toEqual(["连接", "会话列表", "演示数据"]);
  expect(screen.getByRole("tab", { name: "连接" })).toHaveAttribute("aria-selected", "true");
  expect(screen.getByLabelText("connection")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("tab", { name: "会话列表" }));
  expect(screen.getByRole("tab", { name: "会话列表" })).toHaveAttribute("aria-selected", "true");
  expect(screen.getByRole("tabpanel")).toHaveAttribute("aria-labelledby", "aj-settings-tab-sessions");
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
  const sessions = screen.getByRole("tab", { name: "会话列表" });
  expect(sessions).toHaveAttribute("aria-selected", "true");
  expect(document.activeElement).toBe(sessions);
  expect(screen.getByRole("tabpanel")).toHaveAttribute("aria-labelledby", "aj-settings-tab-sessions");
});

test("sort/filter controls persist through the sidebar prefs store and notify subscribers", () => {
  localStorage.clear();
  const seen: unknown[] = [];
  window.addEventListener("agentj:sidebar-prefs-updated", (e) =>
    seen.push((e as CustomEvent<unknown>).detail),
  );
  render(<SettingsDialog onConnect={() => {}} onClose={() => {}} />);
  fireEvent.click(screen.getByRole("tab", { name: "会话列表" }));

  fireEvent.click(screen.getByLabelText("创建时间"));
  expect(loadSidebarPrefs()).toEqual({ sort: "created", filter: "all" });
  expect(JSON.parse(localStorage.getItem(SIDEBAR_PREFS_KEY) ?? "null")).toEqual({
    sort: "created",
    filter: "all",
  });
  expect(seen).toContainEqual({ sort: "created", filter: "all" });

  fireEvent.click(screen.getByLabelText("收藏"));
  expect(loadSidebarPrefs()).toEqual({ sort: "created", filter: "favorites" });
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
