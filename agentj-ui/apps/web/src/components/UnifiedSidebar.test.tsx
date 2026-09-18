import "@testing-library/jest-dom/vitest";
import { test, expect } from "vitest";
import { render, screen, fireEvent, waitFor, within } from "@testing-library/react";
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

function makeClient(sessions: Array<Record<string, unknown>>) {
  const calls: Array<{ method: string; params: Record<string, unknown> }> = [];
  const rpc = {
    request: async (method: string, params: Record<string, unknown> = {}) => {
      calls.push({ method, params });
      if (method === "session/list") return { sessions };
      return null;
    },
    setNotificationHandler: () => {},
  };
  return { calls, client: rpc };
}

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

test("every session row renders a delete button (flat + grouped)", async () => {
  localStorage.clear();
  const { client: c } = makeClient([
    { session_id: "s1", state: "active", preview: "hi", updated_at: String(Date.now()) },
  ]);
  const { container } = render(
    <UnifiedSidebar wsUrl="ws://x" client={c as never} activeSessionId={null} onActiveChange={() => {}} />,
  );
  const flat = container.querySelector('[aria-label="最近会话"]') as HTMLElement;
  await within(flat).findByText("hi");
  const grouped = container.querySelector('[aria-label="session list"]') as HTMLElement;
  expect(within(flat).getAllByLabelText("删除会话").length).toBe(1);
  expect(within(grouped).getAllByLabelText("删除会话").length).toBe(1);
  localStorage.clear();
});

test("per-row delete confirms and removes the session via session/delete", async () => {
  localStorage.clear();
  localStorage.setItem("agentj.favorites", JSON.stringify(["s1"]));
  const removed: string[] = [];
  const { calls, client: c } = makeClient([
    { session_id: "s1", state: "active", preview: "hello", updated_at: String(Date.now()) },
  ]);
  const { container } = render(
    <UnifiedSidebar
      wsUrl="ws://x"
      client={c as never}
      activeSessionId={null}
      onActiveChange={() => {}}
      onSessionRemoved={(id) => removed.push(id)}
    />,
  );
  const flat = container.querySelector('[aria-label="最近会话"]') as HTMLElement;
  await within(flat).findByText("hello");
  fireEvent.click(within(flat).getAllByLabelText("删除会话")[0]);
  const dialog = screen.getByRole("dialog", { name: "删除会话" });
  expect(within(dialog).getByText(/不可恢复/)).toBeInTheDocument();
  fireEvent.click(within(dialog).getByRole("button", { name: "删除" }));
  await waitFor(() => {
    expect(calls.some((call) => call.method === "session/delete" && call.params.session_id === "s1")).toBe(true);
  });
  await waitFor(() => expect(within(flat).queryByText("hello")).toBeNull());
  expect(removed).toEqual(["s1"]);
  expect(JSON.parse(localStorage.getItem("agentj.favorites") ?? "[]")).not.toContain("s1");
  localStorage.clear();
});

test("clear-all deletes every session except the active one", async () => {
  localStorage.clear();
  const removed: string[] = [];
  const { calls, client: c } = makeClient([
    { session_id: "s1", state: "active", preview: "one", updated_at: "1" },
    { session_id: "s2", state: "active", preview: "two", updated_at: "2" },
    { session_id: "s3", state: "active", preview: "three", updated_at: "3" },
  ]);
  const { container } = render(
    <UnifiedSidebar
      wsUrl="ws://x"
      client={c as never}
      activeSessionId="s2"
      onActiveChange={() => {}}
      onSessionRemoved={(id) => removed.push(id)}
    />,
  );
  const flat = container.querySelector('[aria-label="最近会话"]') as HTMLElement;
  await within(flat).findByText("one");
  const cleanup = screen.getByRole("button", { name: "清理会话" });
  await waitFor(() => expect(cleanup).not.toBeDisabled());
  fireEvent.click(cleanup);
  const dialog = screen.getByRole("dialog", { name: "清理会话" });
  expect(within(dialog).getByText(/将删除 2 个会话/)).toBeInTheDocument();
  fireEvent.click(within(dialog).getByRole("button", { name: "删除全部" }));
  await waitFor(() => {
    const deleted = calls.filter((call) => call.method === "session/delete");
    expect(deleted.map((call) => call.params.session_id).sort()).toEqual(["s1", "s3"]);
  });
  await waitFor(() => expect(within(flat).queryByText("two")).not.toBeNull());
  expect(removed.slice().sort()).toEqual(["s1", "s3"]);
  localStorage.clear();
});

test("session/list rows with state deleted are filtered out", async () => {
  localStorage.clear();
  const { client: c } = makeClient([
    { session_id: "alive", state: "active", preview: "alive-preview", updated_at: String(Date.now()) },
    { session_id: "gone", state: "deleted", preview: "gone-preview", updated_at: String(Date.now()) },
  ]);
  const { container } = render(
    <UnifiedSidebar wsUrl="ws://x" client={c as never} activeSessionId={null} onActiveChange={() => {}} />,
  );
  const flat = container.querySelector('[aria-label="最近会话"]') as HTMLElement;
  await within(flat).findByText("alive-preview");
  expect(within(flat).queryByText("gone-preview")).toBeNull();
  expect(container.querySelectorAll('[aria-label="最近会话"] .aj-session-del').length).toBe(1);
  localStorage.clear();
});
