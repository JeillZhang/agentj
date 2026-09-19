import { expect, test, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { render, screen, fireEvent, within, waitFor } from "@testing-library/react";
import TopbarMenu from "./TopbarMenu";
import { createProject, loadProjects, saveProjectDefaults, setCurrentProject } from "../projectStore";

function openMenu() {
  fireEvent.click(screen.getByRole("button", { name: "更多" }));
}

function openProjectFlyout() {
  const trigger = screen.getByRole("menuitem", { name: "项目" });
  fireEvent.mouseEnter(trigger.parentElement!);
  return trigger;
}

test("first-level menu has exactly five items in order and no inline lists", () => {
  localStorage.clear();
  createProject("Alpha");
  localStorage.setItem("agentj.favorites", JSON.stringify(["s-fav"]));
  render(<TopbarMenu onConnect={() => {}} onContinue={() => {}} />);
  openMenu();
  const menu = screen.getByRole("menu");
  for (const name of ["项目", "继续会话…", "收藏会话", "设置…", "帮助"]) {
    expect(within(menu).getByRole("menuitem", { name })).toBeInTheDocument();
  }
  expect(within(menu).getAllByRole("menuitem")).toHaveLength(5);
  expect(within(menu).queryByText("Alpha")).toBeNull();
  expect(within(menu).queryByText("s-fav")).toBeNull();
  localStorage.clear();
});

test("open menu carries no connection status block", () => {
  render(<TopbarMenu onConnect={() => {}} onContinue={() => {}} />);
  openMenu();
  const menu = screen.getByRole("menu");
  expect(within(menu).queryByText(/已连接|未连接/)).toBeNull();
  expect(within(menu).queryByText("deepseek-chat")).toBeNull();
  expect(within(menu).queryByLabelText("连接状态")).toBeNull();
});

test("项目 flyout opens on hover, shows projects with the current one checked, closes on mouse leave", async () => {
  localStorage.clear();
  const alpha = createProject("Alpha");
  setCurrentProject(alpha.id);
  render(<TopbarMenu onConnect={() => {}} onContinue={() => {}} />);
  openMenu();
  const trigger = openProjectFlyout();
  expect(trigger).toHaveAttribute("aria-expanded", "true");
  const flyout = screen.getByRole("menu", { name: "项目" });
  expect(within(flyout).getByRole("menuitemradio", { name: /Inbox/ })).toHaveAttribute("aria-checked", "false");
  const current = within(flyout).getByRole("menuitemradio", { name: /Alpha/ });
  expect(current).toHaveAttribute("aria-checked", "true");
  expect(current).toHaveTextContent("✓");
  fireEvent.mouseLeave(trigger.parentElement!);
  await waitFor(() => expect(screen.queryByRole("menu", { name: "项目" })).toBeNull());
  expect(trigger).toHaveAttribute("aria-expanded", "false");
  localStorage.clear();
});

test("Escape closes the flyout first, then the menu and restores focus", () => {
  localStorage.clear();
  render(<TopbarMenu onConnect={() => {}} onContinue={() => {}} />);
  const triggerButton = screen.getByRole("button", { name: "更多" });
  fireEvent.click(triggerButton);
  const trigger = openProjectFlyout();
  fireEvent.keyDown(document, { key: "Escape" });
  expect(screen.queryByRole("menu", { name: "项目" })).toBeNull();
  expect(screen.getByRole("menu")).toBeInTheDocument();
  expect(document.activeElement).toBe(trigger);
  fireEvent.keyDown(document, { key: "Escape" });
  expect(screen.queryByRole("menu")).toBeNull();
  expect(document.activeElement).toBe(triggerButton);
  localStorage.clear();
});

test("clicking a project persists it, syncs the project event and closes the menu", () => {
  localStorage.clear();
  const syncs: string[] = [];
  window.addEventListener("agentj:current-project-updated", (e) =>
    syncs.push((e as CustomEvent<{ id: string }>).detail.id),
  );
  const beta = createProject("Beta");
  render(<TopbarMenu onConnect={() => {}} onContinue={() => {}} />);
  openMenu();
  fireEvent.click(screen.getByRole("menuitem", { name: "项目" }));
  const flyout = screen.getByRole("menu", { name: "项目" });
  fireEvent.click(within(flyout).getByRole("menuitemradio", { name: /Beta/ }));
  expect(localStorage.getItem("agentj.currentProject")).toBe(beta.id);
  expect(syncs).toContain(beta.id);
  expect(screen.queryByRole("menu")).toBeNull();
  localStorage.clear();
});

test("＋ 添加项目 opens the dialog and creates + selects the project", () => {
  localStorage.clear();
  render(<TopbarMenu onConnect={() => {}} onContinue={() => {}} />);
  openMenu();
  openProjectFlyout();
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

test("收藏会话 flyout lists favorites and empty state when none", () => {
  localStorage.clear();
  const onContinue = vi.fn();
  localStorage.setItem("agentj.favorites", JSON.stringify(["s-1", "s-2"]));
  render(<TopbarMenu onConnect={() => {}} onContinue={onContinue} />);
  openMenu();
  fireEvent.mouseEnter(screen.getByRole("menuitem", { name: "收藏会话" }).parentElement!);
  const flyout = screen.getByRole("menu", { name: "收藏会话" });
  fireEvent.click(within(flyout).getByRole("menuitem", { name: "s-1" }));
  expect(onContinue).toHaveBeenCalledWith("s-1");
  expect(screen.queryByRole("menu")).toBeNull();

  localStorage.setItem("agentj.favorites", "[]");
  openMenu();
  fireEvent.mouseEnter(screen.getByRole("menuitem", { name: "收藏会话" }).parentElement!);
  expect(screen.getByText("暂无收藏")).toBeInTheDocument();
  localStorage.clear();
});

test("设置… opens the settings dialog and connect roundtrips then closes", () => {
  localStorage.clear();
  const onConnect = vi.fn();
  render(<TopbarMenu onConnect={onConnect} onContinue={() => {}} />);
  openMenu();
  fireEvent.click(screen.getByRole("menuitem", { name: "设置…" }));
  const dialog = screen.getByLabelText("设置");
  expect(dialog.parentElement).toBe(document.body);
  const bar = within(dialog).getByLabelText("connection");
  fireEvent.change(within(bar).getByLabelText("Host"), { target: { value: "127.0.0.1" } });
  fireEvent.change(within(bar).getByLabelText("Port"), { target: { value: "4096" } });
  fireEvent.click(within(bar).getByRole("button", { name: /connect/i }));
  expect(onConnect).toHaveBeenCalledWith("127.0.0.1", "4096", "");
  expect(screen.queryByLabelText("设置")).not.toBeInTheDocument();
  localStorage.clear();
});

test("nested dialog inside settings closes one Escape layer at a time", () => {
  localStorage.clear();
  render(<TopbarMenu onConnect={() => {}} onContinue={() => {}} />);
  openMenu();
  fireEvent.click(screen.getByRole("menuitem", { name: "设置…" }));
  const settings = screen.getByLabelText("设置");
  fireEvent.click(within(settings).getByRole("tab", { name: "项目" }));
  fireEvent.click(screen.getByLabelText("编辑项目 Inbox"));
  expect(screen.getByLabelText("编辑项目")).toBeInTheDocument();

  fireEvent.keyDown(document, { key: "Escape" });
  expect(screen.queryByLabelText("编辑项目")).toBeNull();
  expect(screen.getByLabelText("设置")).toBeInTheDocument();

  fireEvent.keyDown(document, { key: "Escape" });
  expect(screen.queryByLabelText("设置")).toBeNull();
  localStorage.clear();
});

test("continue dialog stays unchanged and submits the session id", () => {
  localStorage.clear();
  const onContinue = vi.fn();
  render(<TopbarMenu onConnect={() => {}} onContinue={onContinue} />);
  openMenu();
  fireEvent.click(screen.getByRole("menuitem", { name: "继续会话…" }));
  const dialog = screen.getByLabelText("继续会话");
  expect(dialog.parentElement).toBe(document.body);
  fireEvent.change(within(dialog).getByLabelText(/session id/i), { target: { value: "abc123" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "继续" }));
  expect(onContinue).toHaveBeenCalledWith("abc123");
  expect(screen.queryByLabelText("继续会话")).not.toBeInTheDocument();
  localStorage.clear();
});

test("outside mousedown and 帮助 both close the menu", () => {
  localStorage.clear();
  render(<TopbarMenu onConnect={() => {}} onContinue={() => {}} />);
  openMenu();
  fireEvent.mouseDown(document.body);
  expect(screen.queryByRole("menu")).toBeNull();

  openMenu();
  fireEvent.click(screen.getByRole("menuitem", { name: "帮助" }));
  expect(screen.queryByRole("menu")).toBeNull();
  localStorage.clear();
});

test("prefills and submits the work dir for new projects", () => {
  localStorage.clear();
  saveProjectDefaults({ workDir: "/repo/default" });
  render(<TopbarMenu onConnect={() => {}} onContinue={() => {}} />);
  fireEvent.click(screen.getByRole("button", { name: "更多" }));
  fireEvent.mouseEnter(screen.getByRole("menuitem", { name: /项目/ }));
  fireEvent.click(screen.getByRole("menuitem", { name: "＋ 添加项目" }));
  const dir = screen.getByLabelText("工作目录") as HTMLInputElement;
  expect(dir.value).toBe("/repo/default");
  fireEvent.change(screen.getByLabelText("项目名称"), { target: { value: "演示" } });
  fireEvent.change(dir, { target: { value: "/repo/demo" } });
  fireEvent.click(screen.getByRole("button", { name: "添加" }));
  const created = loadProjects().find((p) => p.name === "演示");
  expect(created?.workDir).toBe("/repo/demo");
});

test("normalizes an empty work dir to null on project create", () => {
  localStorage.clear();
  render(<TopbarMenu onConnect={() => {}} onContinue={() => {}} />);
  fireEvent.click(screen.getByRole("button", { name: "更多" }));
  fireEvent.mouseEnter(screen.getByRole("menuitem", { name: /项目/ }));
  fireEvent.click(screen.getByRole("menuitem", { name: "＋ 添加项目" }));
  expect((screen.getByLabelText("工作目录") as HTMLInputElement).value).toBe("");
  fireEvent.change(screen.getByLabelText("项目名称"), { target: { value: "X" } });
  fireEvent.click(screen.getByRole("button", { name: "添加" }));
  const created = loadProjects().find((p) => p.name === "X");
  expect(created?.workDir).toBeNull();
});
