import "@testing-library/jest-dom/vitest";
import { test, it, expect, vi } from "vitest";
import { render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import UnifiedSidebar from "./UnifiedSidebar";
import TopbarMenu from "./TopbarMenu";
import { createProject, setCurrentProject, updateProject } from "../projectStore";

// The sidebar scope drives FileTree through its imperative handle; mock it so the
// `r` (refresh) and files-tab `j`/`k` (moveBy) wiring can be asserted directly.
const fileTreeSpies = vi.hoisted(() => ({ refresh: vi.fn(), moveBy: vi.fn() }));

vi.mock("./FileTree", async () => {
  const React = await import("react");
  const FileTreeMock = React.forwardRef<unknown, Record<string, unknown>>(function FileTreeMock(_props, ref) {
    React.useImperativeHandle(ref, () => ({
      moveBy: fileTreeSpies.moveBy,
      refresh: fileTreeSpies.refresh,
    }));
    return React.createElement("div", { "data-testid": "file-tree-mock", tabIndex: -1 });
  });
  return { default: FileTreeMock };
});

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

function makeClient(sessions: Array<Record<string, unknown>>, newSessionId?: string) {
  const calls: Array<{ method: string; params: Record<string, unknown> }> = [];
  const rpc = {
    request: async (method: string, params: Record<string, unknown> = {}) => {
      calls.push({ method, params });
      if (method === "session/list") return { sessions };
      if (method === "session/new" && newSessionId) return { session_id: newSessionId };
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
  expect(container.querySelector(".aj-sidebar__footer")).toBeNull();
  expect(container.querySelector(".aj-project-group")).toBeNull();
  expect(screen.queryByText("项目")).toBeNull();
  expect(screen.queryByText("+ 添加项目")).toBeNull();
  expect(screen.queryByText("＋ 添加项目")).toBeNull();
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
  fireEvent.click(container.querySelector(".aj-star") as HTMLElement);
  expect(JSON.parse(localStorage.getItem("agentj.favorites") ?? "[]")).toContain("s1");
});

test("clicking a session row leaves favorites untouched; only the star favorites it", async () => {
  localStorage.clear();
  const onActive = vi.fn();
  const { container } = render(
    <UnifiedSidebar wsUrl="ws://x" client={client as never} activeSessionId={null} onActiveChange={onActive} />,
  );
  const flat = container.querySelector('[aria-label="最近会话"]') as HTMLElement;
  const title = await within(flat).findByText("hi");
  fireEvent.click(title);
  await waitFor(() => expect(onActive).toHaveBeenCalledWith("s1"));
  expect(JSON.parse(localStorage.getItem("agentj.favorites") ?? "[]")).toEqual([]);
  expect(flat.querySelector(".aj-star")).toHaveAttribute("aria-pressed", "false");

  fireEvent.click(flat.querySelector(".aj-star") as HTMLElement);
  expect(JSON.parse(localStorage.getItem("agentj.favorites") ?? "[]")).toEqual(["s1"]);
  localStorage.clear();
});

test("sidebar no longer renders the project section or add-project dialog", () => {
  localStorage.clear();
  const { container } = render(
    <UnifiedSidebar wsUrl="ws://x" client={client as never} activeSessionId={null} onActiveChange={() => {}} />,
  );
  expect(container.querySelector(".aj-sidebar__footer")).toBeNull();
  expect(container.querySelector(".aj-project-group")).toBeNull();
  expect(container.querySelector(".section-label")).toBeNull();
  expect(screen.queryByText("项目")).toBeNull();
  expect(screen.queryByText("＋ 添加项目")).toBeNull();
  expect(screen.queryByLabelText("添加项目")).toBeNull();
  localStorage.clear();
});

test("every session row renders a delete button", async () => {
  localStorage.clear();
  const { client: c } = makeClient([
    { session_id: "s1", state: "active", preview: "hi", updated_at: String(Date.now()) },
  ]);
  const { container } = render(
    <UnifiedSidebar wsUrl="ws://x" client={c as never} activeSessionId={null} onActiveChange={() => {}} />,
  );
  const flat = container.querySelector('[aria-label="最近会话"]') as HTMLElement;
  await within(flat).findByText("hi");
  expect(within(flat).getAllByLabelText("删除会话").length).toBe(1);
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

it("opens the cleanup dialog from the session list menu", async () => {
  const client = {
    request: vi.fn(async () => ({
      sessions: [
        { session_id: "a", preview: "one" },
        { session_id: "b", preview: "two" },
      ],
    })),
  };
  render(
    <UnifiedSidebar wsUrl="ws://x" client={client} activeSessionId={null} onActiveChange={() => {}} />,
  );
  await screen.findByText("one");
  fireEvent.click(screen.getByRole("button", { name: "会话列表菜单" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "清理所有会话…" }));
  expect(await screen.findByText(/将删除/)).toBeTruthy();
  localStorage.clear();
});

test("session list menu disables clear-all when nothing is deletable", async () => {
  localStorage.clear();
  const { client: c } = makeClient([
    { session_id: "only", state: "active", preview: "solo", updated_at: "1" },
  ]);
  render(
    <UnifiedSidebar wsUrl="ws://x" client={c as never} activeSessionId="only" onActiveChange={() => {}} />,
  );
  await screen.findByText("solo");
  fireEvent.click(screen.getByRole("button", { name: "会话列表菜单" }));
  expect(screen.getByRole("menuitem", { name: "清理所有会话…" })).toHaveAttribute(
    "aria-disabled",
    "true",
  );
  localStorage.clear();
});

test("clear-all from the menu deletes every session except the active one", async () => {
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
  fireEvent.click(screen.getByRole("button", { name: "会话列表菜单" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "清理所有会话…" }));
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

test("project picked in TopbarMenu is inherited by the next new session via agentj:current-project-updated", async () => {
  localStorage.clear();
  const qa = createProject("QA");
  const { client: c } = makeClient([], "s-new");
  render(
    <>
      <TopbarMenu onConnect={() => {}} onContinue={() => {}} />
      <UnifiedSidebar wsUrl="ws://x" client={c as never} activeSessionId={null} onActiveChange={() => {}} />
    </>,
  );
  fireEvent.click(screen.getByRole("button", { name: "更多" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "项目" }));
  const flyout = screen.getByRole("menu", { name: "项目" });
  fireEvent.click(within(flyout).getByRole("menuitemradio", { name: /QA/ }));
  fireEvent.click(screen.getByRole("button", { name: "+ 新建会话" }));
  await waitFor(() => {
    const map = JSON.parse(localStorage.getItem("agentj.projectMap") ?? "{}") as Record<string, string>;
    expect(map["s-new"]).toBe(qa.id);
  });
  const map = JSON.parse(localStorage.getItem("agentj.projectMap") ?? "{}") as Record<string, string>;
  expect(map["s-new"]).not.toBe("default");
  localStorage.clear();
});

test("sends project_root when the project defines a work dir", async () => {
  localStorage.clear();
  const alpha = createProject("Alpha");
  updateProject(alpha.id, { workDir: "/repo/alpha" });
  setCurrentProject(alpha.id);
  const requests: Array<{ method: string; params: unknown }> = [];
  const client = {
    request: vi.fn(async (method: string, params: unknown) => {
      requests.push({ method, params });
      if (method === "session/new") return { session_id: "s1" };
      return {};
    }),
  };
  render(
    <UnifiedSidebar wsUrl="ws://x" client={client} activeSessionId={null} onActiveChange={() => {}} />,
  );
  fireEvent.click(screen.getByRole("button", { name: /新建会话/ }));
  await waitFor(() => expect(requests.some((r) => r.method === "session/new")).toBe(true));
  expect(requests.find((r) => r.method === "session/new")?.params).toEqual({
    project_root: "/repo/alpha",
  });
});

test("omits project_root when the project has no work dir", async () => {
  localStorage.clear();
  const beta = createProject("Beta");
  setCurrentProject(beta.id);
  const requests: Array<{ method: string; params: unknown }> = [];
  const client = {
    request: vi.fn(async (method: string, params: unknown) => {
      requests.push({ method, params });
      if (method === "session/new") return { session_id: "s2" };
      return {};
    }),
  };
  render(
    <UnifiedSidebar wsUrl="ws://x" client={client} activeSessionId={null} onActiveChange={() => {}} />,
  );
  fireEvent.click(screen.getByRole("button", { name: /新建会话/ }));
  await waitFor(() => expect(requests.some((r) => r.method === "session/new")).toBe(true));
  expect(requests.find((r) => r.method === "session/new")?.params).toEqual({});
});

test("surfaces session/new failures via the error banner", async () => {
  localStorage.clear();
  const alpha = createProject("Alpha");
  updateProject(alpha.id, { workDir: "/repo/alpha" });
  setCurrentProject(alpha.id);
  const client = {
    request: vi.fn(async (method: string) => {
      if (method === "session/new") throw new Error("-32807 PathNotAllowed: /repo/alpha");
      return {};
    }),
  };
  render(
    <UnifiedSidebar wsUrl="ws://x" client={client} activeSessionId={null} onActiveChange={() => {}} />,
  );
  fireEvent.click(screen.getByRole("button", { name: /新建会话/ }));
  expect(await screen.findByRole("alert")).toBeTruthy();
});

const rovingSessions = [
  { session_id: "s1", state: "active", preview: "one", updated_at: "3" },
  { session_id: "s2", state: "active", preview: "two", updated_at: "2" },
  { session_id: "s3", state: "active", preview: "three", updated_at: "1" },
];

async function renderRovingSidebar() {
  localStorage.clear();
  const { client: c } = makeClient(rovingSessions);
  const view = render(
    <UnifiedSidebar wsUrl="ws://x" client={c as never} activeSessionId={null} onActiveChange={() => {}} />,
  );
  const flat = view.container.querySelector('[aria-label="最近会话"]') as HTMLElement;
  await within(flat).findByText("one");
  return { flat, rows: flat.querySelectorAll<HTMLElement>(".aj-sess") };
}

it("uses a roving tabindex so only the selected session row is tabbable", async () => {
  const { rows } = await renderRovingSidebar();
  expect(rows.length).toBe(3);
  await waitFor(() => expect(rows[0]).toHaveAttribute("tabindex", "0"));
  expect(rows[1]).toHaveAttribute("tabindex", "-1");
  expect(rows[2]).toHaveAttribute("tabindex", "-1");
  localStorage.clear();
});

it("moves focus with arrow keys across session rows", async () => {
  const { rows } = await renderRovingSidebar();
  rows[0].focus();
  fireEvent.keyDown(rows[0], { key: "ArrowDown" });
  expect(document.activeElement).toBe(rows[1]);
  await waitFor(() => expect(rows[1]).toHaveAttribute("tabindex", "0"));
  expect(rows[0]).toHaveAttribute("tabindex", "-1");
  fireEvent.keyDown(rows[1], { key: "ArrowUp" });
  expect(document.activeElement).toBe(rows[0]);
  localStorage.clear();
});

it("continues the focused session on Enter", async () => {
  localStorage.clear();
  const onActive = vi.fn();
  const { calls, client: c } = makeClient(rovingSessions);
  const { container } = render(
    <UnifiedSidebar wsUrl="ws://x" client={c as never} activeSessionId={null} onActiveChange={onActive} />,
  );
  const flat = container.querySelector('[aria-label="最近会话"]') as HTMLElement;
  const title = await within(flat).findByText("two");
  const row = title.closest(".aj-sess") as HTMLElement;
  row.focus();
  fireEvent.keyDown(row, { key: "Enter" });
  await waitFor(() => expect(onActive).toHaveBeenCalledWith("s2"));
  const statusCalls = calls.filter((call) => call.method === "session/status");
  expect(statusCalls.some((call) => call.params.session_id === "s2")).toBe(true);
  localStorage.clear();
});

async function renderShortcutSidebar() {
  localStorage.clear();
  fileTreeSpies.refresh.mockClear();
  fileTreeSpies.moveBy.mockClear();
  const { calls, client: c } = makeClient(rovingSessions, "s-new");
  const view = render(
    <>
      <UnifiedSidebar wsUrl="ws://x" client={c as never} activeSessionId={null} onActiveChange={() => {}} />
      <input id="aj-prompt" />
    </>,
  );
  const flat = view.container.querySelector('[aria-label="最近会话"]') as HTMLElement;
  await within(flat).findByText("one");
  return { calls, flat };
}

it("toggles the sidebar tab with f and persists it", async () => {
  const { flat } = await renderShortcutSidebar();
  (flat.querySelector(".aj-sess") as HTMLElement).focus();
  fireEvent.keyDown(document, { key: "f" });
  expect(screen.getByRole("tab", { name: "文件" })).toHaveAttribute("aria-selected", "true");
  expect(localStorage.getItem("agentj.layout.leftTab")).toBe("files");
});

it("returns focus to the composer with Escape", async () => {
  const { flat } = await renderShortcutSidebar();
  (flat.querySelector(".aj-sess") as HTMLElement).focus();
  fireEvent.keyDown(document, { key: "Escape" });
  expect(document.activeElement?.id).toBe("aj-prompt");
});

it("refreshes the file tree with r on the files tab", async () => {
  const { flat } = await renderShortcutSidebar();
  (flat.querySelector(".aj-sess") as HTMLElement).focus();
  fireEvent.keyDown(document, { key: "f" });
  screen.getByTestId("file-tree-mock").focus();
  fireEvent.keyDown(document, { key: "r" });
  expect(fileTreeSpies.refresh).toHaveBeenCalledTimes(1);
});

it("creates a new session with n from the sidebar scope", async () => {
  const { calls, flat } = await renderShortcutSidebar();
  (flat.querySelector(".aj-sess") as HTMLElement).focus();
  fireEvent.keyDown(document, { key: "n" });
  await waitFor(() => expect(calls.some((call) => call.method === "session/new")).toBe(true));
});

it("moves the session selection with j and k", async () => {
  const { flat } = await renderShortcutSidebar();
  const rows = flat.querySelectorAll<HTMLElement>(".aj-sess");
  rows[0].focus();
  fireEvent.keyDown(document, { key: "j" });
  expect(document.activeElement).toBe(rows[1]);
  fireEvent.keyDown(document, { key: "k" });
  expect(document.activeElement).toBe(rows[0]);
});

it("moves the file tree selection with j and k on the files tab", async () => {
  const { flat } = await renderShortcutSidebar();
  (flat.querySelector(".aj-sess") as HTMLElement).focus();
  fireEvent.keyDown(document, { key: "f" });
  screen.getByTestId("file-tree-mock").focus();
  fireEvent.keyDown(document, { key: "j" });
  expect(fileTreeSpies.moveBy).toHaveBeenCalledWith(1);
  fireEvent.keyDown(document, { key: "k" });
  expect(fileTreeSpies.moveBy).toHaveBeenCalledWith(-1);
});

it("fires no sidebar shortcut while typing in the composer", async () => {
  const { calls, flat } = await renderShortcutSidebar();
  const prompt = document.getElementById("aj-prompt") as HTMLInputElement;
  prompt.focus();
  for (const key of ["f", "n", "r", "j", "k"]) {
    fireEvent.keyDown(prompt, { key });
  }
  expect(screen.getByRole("tab", { name: "会话" })).toHaveAttribute("aria-selected", "true");
  expect(calls.some((call) => call.method === "session/new")).toBe(false);
  expect(fileTreeSpies.refresh).not.toHaveBeenCalled();
  expect(fileTreeSpies.moveBy).not.toHaveBeenCalled();
  expect(flat.querySelectorAll(".aj-sess").length).toBe(3);
});

it("ignores sidebar shortcuts when focus is outside the sidebar", async () => {
  localStorage.clear();
  const { client: c } = makeClient(rovingSessions);
  render(
    <>
      <UnifiedSidebar wsUrl="ws://x" client={c as never} activeSessionId={null} onActiveChange={() => {}} />
      <input id="aj-prompt" />
    </>,
  );
  document.getElementById("aj-prompt")!.focus();
  fireEvent.keyDown(document, { key: "f" });
  expect(screen.getByRole("tab", { name: "会话" })).toHaveAttribute("aria-selected", "true");
});
