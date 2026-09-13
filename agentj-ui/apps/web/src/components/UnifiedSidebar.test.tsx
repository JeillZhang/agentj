import "@testing-library/jest-dom/vitest";
import { test, expect } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
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
  await screen.findByText("hi");
  fireEvent.click(container.querySelector(".aj-star") as HTMLElement);
  expect(JSON.parse(localStorage.getItem("agentj.favorites") ?? "[]")).toContain("s1");
});
