import "@testing-library/jest-dom/vitest";
import { test, expect } from "vitest";
import { render, screen, fireEvent, within } from "@testing-library/react";
import UnifiedSidebar from "./UnifiedSidebar";

const client = {
  request: async (m: string) =>
    m === "session/list"
      ? {
          sessions: [
            { session_id: "s1", state: "active", preview: "hi", updated_at: String(Date.now()) },
          ],
        }
      : null,
  setNotificationHandler: () => {},
};

test("tabs, no legacy widgets, files tab persistence", async () => {
  const { container } = render(
    <UnifiedSidebar wsUrl="ws://x" client={client as never} activeSessionId={null} onActiveChange={() => {}} />,
  );
  expect(screen.getByRole("tab", { name: "会话" })).toBeInTheDocument();
  expect(screen.queryByText("Threads")).toBeNull();
  expect(screen.queryByLabelText("Settings")).toBeNull();
  expect(container.querySelector(".aj-sidebar__footer")?.textContent).toContain("添加项目");
  fireEvent.click(screen.getByRole("tab", { name: "文件" }));
  expect(localStorage.getItem("agentj.layout.leftTab")).toBe("files");
  localStorage.clear();
});

test("restores the persisted tab on mount", () => {
  localStorage.setItem("agentj.layout.leftTab", "files");
  render(
    <UnifiedSidebar wsUrl="ws://x" client={client as never} activeSessionId={null} onActiveChange={() => {}} />,
  );
  expect(screen.getByRole("tab", { name: "文件" })).toHaveAttribute("aria-selected", "true");
  localStorage.clear();
});

test("flat list renders sessions and star toggle persists", async () => {
  const { container } = render(
    <UnifiedSidebar wsUrl="ws://x" client={client as never} activeSessionId={null} onActiveChange={() => {}} />,
  );
  const flat = container.querySelector('[aria-label="最近会话"]') as HTMLElement;
  await within(flat).findByText("hi");
  const grouped = container.querySelector('[aria-label="session list"]') as HTMLElement;
  expect(within(grouped).getByText("hi")).toBeInTheDocument();
  fireEvent.click(container.querySelector(".aj-star") as HTMLElement);
  expect(JSON.parse(localStorage.getItem("agentj.favorites") ?? "[]")).toContain("s1");
});

test("add-project dialog creates a project from the footer button", async () => {
  localStorage.clear();
  const { container } = render(
    <UnifiedSidebar wsUrl="ws://x" client={client as never} activeSessionId={null} onActiveChange={() => {}} />,
  );
  fireEvent.click(screen.getByRole("button", { name: "+ 添加项目" }));
  const dialog = screen.getByLabelText("添加项目");
  expect(dialog.parentElement).toBe(document.body);

  const addButton = within(dialog).getByRole("button", { name: "添加" });
  expect(addButton).toBeDisabled();
  fireEvent.change(within(dialog).getByLabelText("项目名称"), { target: { value: "新项目" } });
  fireEvent.click(addButton);

  expect(screen.queryByLabelText("添加项目")).not.toBeInTheDocument();
  expect(localStorage.getItem("agentj.projects") ?? "").toContain("新项目");
  await within(container).findByText("新项目");
  localStorage.clear();
});

test("add-project dialog closes on backdrop mousedown and cancel", () => {
  localStorage.clear();
  render(
    <UnifiedSidebar wsUrl="ws://x" client={client as never} activeSessionId={null} onActiveChange={() => {}} />,
  );
  fireEvent.click(screen.getByRole("button", { name: "+ 添加项目" }));
  fireEvent.mouseDown(screen.getByLabelText("添加项目"));
  expect(screen.queryByLabelText("添加项目")).not.toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "+ 添加项目" }));
  fireEvent.click(within(screen.getByLabelText("添加项目")).getByRole("button", { name: "取消" }));
  expect(screen.queryByLabelText("添加项目")).not.toBeInTheDocument();
  localStorage.clear();
});
