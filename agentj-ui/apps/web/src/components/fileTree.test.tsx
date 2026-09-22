import "@testing-library/jest-dom/vitest";
import { expect, test, vi } from "vitest";
import { render, screen, fireEvent, waitFor, act } from "@testing-library/react";
import { createRef } from "react";
import type { RpcClient } from "@agentj/sdk";
import FileTree from "./FileTree";
import type { FileTreeHandle } from "./FileTree";
import { visiblePaths } from "./fileTreeNav";
import type { NavEntry } from "./fileTreeNav";

type Call = { method: string; params: Record<string, unknown> };

type RawEntry = { name: string; path: string; kind: string; git_status?: string | null };

const ROOT: RawEntry[] = [
  { name: "src", path: "src", kind: "dir", git_status: null },
  { name: "README.md", path: "README.md", kind: "file", git_status: null },
];

const CHILDREN: RawEntry[] = [
  { name: "App.tsx", path: "src/App.tsx", kind: "file", git_status: "M" },
  { name: "new.ts", path: "src/new.ts", kind: "file", git_status: "A" },
  { name: "legacy.ts", path: "src/legacy.ts", kind: "file", git_status: "D" },
  { name: "untracked.ts", path: "src/untracked.ts", kind: "file", git_status: "U" },
];

/** Minimal RpcClient double that records every call and answers by requested path. */
function mockClient(
  responder: (path: string | null, call: Call) => unknown,
  calls: Call[] = [],
): RpcClient {
  return {
    request: async (method: string, params: Record<string, unknown> = {}) => {
      const call: Call = { method, params };
      calls.push(call);
      return responder((params.path as string | undefined) ?? null, call);
    },
  } as unknown as RpcClient;
}

/** `vi.fn`-backed client double so tests can assert exact request arguments. */
function fakeClient(byPath: Record<string, RawEntry[]>) {
  const request = vi.fn(async (_method: string, params: Record<string, unknown> = {}) => {
    const key = (params.path as string | undefined) ?? "";
    return { entries: byPath[key] ?? [], truncated: false };
  });
  return { client: { request } as unknown as RpcClient, request };
}

/** Returns the `role="tree"` container or throws while the tree is still loading. */
function getTree(container: HTMLElement): HTMLElement {
  const tree = container.querySelector<HTMLElement>('[role="tree"]');
  if (!tree) throw new Error("tree not rendered");
  return tree;
}

test("lazy-loads a directory on expand and caches children", async () => {
  const calls: Call[] = [];
  const client = mockClient((path) => ({ entries: path ? CHILDREN : ROOT, truncated: false }), calls);

  render(<FileTree client={client} sessionId="s1" demo={false} />);
  await screen.findByText("src");
  expect(calls).toHaveLength(1);
  expect(calls[0].method).toBe("file/list");
  expect(calls[0].params.session_id).toBe("s1");
  expect(calls[0].params.depth).toBe(1);
  expect("path" in calls[0].params).toBe(false);
  expect(screen.queryByText("App.tsx")).toBeNull();

  fireEvent.click(screen.getByRole("treeitem", { name: /src/ }));
  await screen.findByText("App.tsx");
  expect(calls).toHaveLength(2);
  expect(calls[1].method).toBe("file/list");
  expect(calls[1].params.session_id).toBe("s1");
  expect(calls[1].params.path).toBe("src");
  expect(calls[1].params.depth).toBe(1);

  // collapse then re-expand reuses the cached children (no third request)
  fireEvent.click(screen.getByRole("treeitem", { name: /src/ }));
  expect(screen.queryByText("App.tsx")).toBeNull();
  fireEvent.click(screen.getByRole("treeitem", { name: /src/ }));
  await screen.findByText("App.tsx");
  expect(calls).toHaveLength(2);
});

test("shows retry after failure and re-requests the same path", async () => {
  const calls: Call[] = [];
  let failOnce = true;
  const client = mockClient((path) => {
    if (path === "src" && failOnce) {
      failOnce = false;
      throw new Error("boom");
    }
    return { entries: path ? CHILDREN : ROOT, truncated: false };
  }, calls);

  render(<FileTree client={client} sessionId="s1" demo={false} />);
  await screen.findByText("src");
  fireEvent.click(screen.getByRole("treeitem", { name: /src/ }));
  await screen.findByText("加载失败");

  fireEvent.click(screen.getByRole("button", { name: "重试" }));
  await screen.findByText("App.tsx");
  const srcCalls = calls.filter((c) => c.params.path === "src");
  expect(srcCalls).toHaveLength(2);
  expect(srcCalls[0].params.depth).toBe(1);
});

test("renders M/A/U/D git letters from git_status and no dot", async () => {
  const calls: Call[] = [];
  const client = mockClient((path) => ({ entries: path ? CHILDREN : ROOT, truncated: false }), calls);
  const { container } = render(<FileTree client={client} sessionId="s1" demo={false} />);
  await screen.findByText("src");
  fireEvent.click(screen.getByRole("treeitem", { name: /src/ }));
  await screen.findByText("App.tsx");
  expect(container.querySelector(".status-M")).not.toBeNull();
  expect(container.querySelector(".status-A")).not.toBeNull();
  expect(container.querySelector(".status-U")).not.toBeNull();
  expect(container.querySelector(".status-D")).not.toBeNull();
  expect(container.querySelector(".dot")).toBeNull();
});

test("dir rows show the aggregate status letter", async () => {
  const root: RawEntry[] = [{ name: "src", path: "src", kind: "dir", git_status: "D" }];
  const client = mockClient(() => ({ entries: root, truncated: false }));
  const { container } = render(<FileTree client={client} sessionId="s1" demo={false} />);
  await screen.findByText("src");
  expect(container.querySelector(".trow-dir .status-D")).not.toBeNull();
  expect(container.querySelector(".trow-dir .dot")).toBeNull();
});

test("footer counts tracked-status changes only and keeps the expand glyph", async () => {
  const entries: RawEntry[] = [
    { name: "a.ts", path: "a.ts", kind: "file", git_status: "M" },
    { name: "b.ts", path: "b.ts", kind: "file", git_status: "U" },
  ];
  const client = mockClient(() => ({ entries, truncated: false }));
  render(<FileTree client={client} sessionId="s1" demo={false} />);
  await screen.findByText("a.ts");
  expect(screen.getByText(/2 个文件 · 1 处变更/)).toBeInTheDocument();
  expect(screen.getByText("⤢ 展开全部")).toBeInTheDocument();
});

test("empty directory shows 空目录", async () => {
  const client = mockClient(() => ({ entries: [], truncated: false }));
  render(<FileTree client={client} sessionId="s1" demo={false} />);
  await screen.findByText("空目录");
  expect(screen.getByText(/0 个文件 · 0 处变更/)).toBeInTheDocument();
});

test("no sessionId shows 未选择会话 without requesting", () => {
  const calls: Call[] = [];
  const client = mockClient(() => ROOT, calls);
  render(<FileTree client={client} sessionId={null} demo={false} />);
  expect(screen.getByText("未选择会话")).toBeInTheDocument();
  expect(calls).toHaveLength(0);
});

test("loading root shows 加载中…", () => {
  const client = {
    request: () => new Promise(() => {}),
  } as unknown as RpcClient;
  render(<FileTree client={client} sessionId="s1" demo={false} />);
  expect(screen.getByText("加载中…")).toBeInTheDocument();
});

test("uses demo fixtures without requests", () => {
  const calls: Call[] = [];
  const client = mockClient(() => null, calls);
  render(<FileTree client={client} sessionId="s1" demo />);
  expect(screen.getByText(/10 个文件 · 3 处变更/)).toBeInTheDocument();
  expect(screen.queryByText("apps/")).toBeNull();
  fireEvent.click(screen.getByRole("treeitem", { name: /agentj-ui/ }));
  expect(screen.getByText("apps/")).toBeInTheDocument();
  expect(calls).toHaveLength(0);
});

test("dir icon swaps Folder/FolderOpen with the expanded state", async () => {
  const client = mockClient((path) => ({ entries: path ? CHILDREN : ROOT, truncated: false }));
  render(<FileTree client={client} sessionId="s1" demo={false} />);
  await screen.findByText("src");

  const collapsed = screen.getByRole("treeitem", { name: /src/ });
  expect(collapsed.querySelector(".dir-icon--closed")).not.toBeNull();
  expect(collapsed.querySelector(".dir-icon--open")).toBeNull();

  fireEvent.click(collapsed);
  await screen.findByText("App.tsx");

  const expanded = screen.getByRole("treeitem", { name: /src/ });
  expect(expanded).toHaveAttribute("aria-expanded", "true");
  expect(expanded.querySelector(".dir-icon--open")).not.toBeNull();
  expect(expanded.querySelector(".dir-icon--closed")).toBeNull();
  expect(expanded.querySelector(".chev")).not.toBeNull();
});

test("applies per-category icon modifiers to files", async () => {
  const entries: RawEntry[] = [
    { name: "app.ts", path: "app.ts", kind: "file", git_status: null },
    { name: "data.json", path: "data.json", kind: "file", git_status: null },
    { name: "readme.md", path: "readme.md", kind: "file", git_status: null },
    { name: "logo.png", path: "logo.png", kind: "file", git_status: null },
    { name: "data.xyz", path: "data.xyz", kind: "file", git_status: null },
  ];
  const client = mockClient(() => ({ entries, truncated: false }));
  const { container } = render(<FileTree client={client} sessionId="s1" demo={false} />);
  await screen.findByText("app.ts");

  expect(container.querySelector(".file-icon--code")).not.toBeNull();
  expect(container.querySelector(".file-icon--json")).not.toBeNull();
  expect(container.querySelector(".file-icon--doc")).not.toBeNull();
  expect(container.querySelector(".file-icon--image")).not.toBeNull();
  expect(container.querySelector(".file-icon--default")).not.toBeNull();
});

// --- T20: keyboard navigation -------------------------------------------------

const NAV_ENTRIES: Map<string, NavEntry[]> = new Map([
  [
    "",
    [
      { path: "src", kind: "dir" },
      { path: "README.md", kind: "file" },
    ],
  ],
  [
    "src",
    [
      { path: "src/App.tsx", kind: "file" },
      { path: "src/utils", kind: "dir" },
    ],
  ],
  ["src/utils", [{ path: "src/utils/helper.ts", kind: "file" }]],
]);

const NAV_ROOT: RawEntry[] = [
  { name: "src", path: "src", kind: "dir", git_status: null },
  { name: "README.md", path: "README.md", kind: "file", git_status: null },
];

const NAV_CHILDREN: RawEntry[] = [
  { name: "App.tsx", path: "src/App.tsx", kind: "file", git_status: null },
  { name: "utils", path: "src/utils", kind: "dir", git_status: null },
];

const NAV_NESTED: RawEntry[] = [
  { name: "helper.ts", path: "src/utils/helper.ts", kind: "file", git_status: null },
];

test("visiblePaths walks depth-first with dirs before recursion and only expanded children", () => {
  expect(visiblePaths(NAV_ENTRIES, new Set())).toEqual(["src", "README.md"]);
  expect(visiblePaths(NAV_ENTRIES, new Set(["src"]))).toEqual([
    "src",
    "src/App.tsx",
    "src/utils",
    "README.md",
  ]);
  expect(visiblePaths(NAV_ENTRIES, new Set(["src", "src/utils"]))).toEqual([
    "src",
    "src/App.tsx",
    "src/utils",
    "src/utils/helper.ts",
    "README.md",
  ]);
  expect(visiblePaths(new Map(), new Set(["src"]))).toEqual([]);
});

test("moves selection with ArrowDown and toggles with Enter", async () => {
  const { client, request } = fakeClient({ "": NAV_ROOT, src: NAV_CHILDREN });
  const { getByText, container } = render(<FileTree client={client} sessionId="s1" demo={false} />);
  await waitFor(() => expect(getByText("src")).toBeTruthy());
  getByText("src").focus();
  fireEvent.keyDown(getTree(container), { key: "ArrowDown" });
  expect(document.activeElement?.textContent).toContain("README.md");
  fireEvent.keyDown(getTree(container), { key: "ArrowUp" });
  expect(document.activeElement?.textContent).toContain("src");
  fireEvent.keyDown(getTree(container), { key: "Enter" });
  fireEvent.keyDown(getTree(container), { key: "End" });
  expect(document.activeElement?.textContent).toContain("README.md");
  await waitFor(() =>
    expect(request).toHaveBeenCalledWith("file/list", expect.objectContaining({ path: "src" })),
  );
});

test("rows expose role=treeitem with aria-selected and dirs carry aria-expanded", async () => {
  const { client } = fakeClient({ "": NAV_ROOT, src: NAV_CHILDREN });
  const { container } = render(<FileTree client={client} sessionId="s1" demo={false} />);
  await screen.findByText("src");
  const tree = getTree(container);
  expect(tree).toBeTruthy();
  expect(container.querySelectorAll('[role="treeitem"]')).toHaveLength(2);

  const dir = screen.getByRole("treeitem", { name: /^src$/ });
  expect(dir).toHaveAttribute("aria-expanded", "false");
  expect(dir).toHaveAttribute("aria-selected", "false");
  expect(dir).toHaveAttribute("tabindex", "0");

  fireEvent.keyDown(tree, { key: "End" });
  const file = screen.getByRole("treeitem", { name: /README\.md/ });
  expect(file).toHaveAttribute("aria-selected", "true");
  expect(file).toHaveAttribute("tabindex", "0");
  expect(dir).toHaveAttribute("tabindex", "-1");

  fireEvent.click(dir);
  await screen.findByText("App.tsx");
  expect(dir).toHaveAttribute("aria-expanded", "true");
});

test("Home and End jump to the first and last visible row", async () => {
  const { client } = fakeClient({ "": NAV_ROOT, src: NAV_CHILDREN });
  const { container } = render(<FileTree client={client} sessionId="s1" demo={false} />);
  await screen.findByText("src");
  const tree = getTree(container);

  fireEvent.keyDown(tree, { key: "End" });
  expect(document.activeElement?.textContent).toContain("README.md");
  fireEvent.keyDown(tree, { key: "Home" });
  expect(document.activeElement?.textContent).toContain("src");
});

test("Space toggles, ArrowRight expands and ArrowLeft collapses or jumps to the parent", async () => {
  const { client } = fakeClient({ "": NAV_ROOT, src: NAV_CHILDREN, "src/utils": NAV_NESTED });
  const { container } = render(<FileTree client={client} sessionId="s1" demo={false} />);
  await screen.findByText("src");
  const tree = getTree(container);

  fireEvent.keyDown(tree, { key: "ArrowDown" });
  fireEvent.keyDown(tree, { key: "ArrowUp" });
  fireEvent.keyDown(tree, { key: "ArrowRight" });
  await screen.findByText("App.tsx");
  expect(screen.getByRole("treeitem", { name: /^src$/ })).toHaveAttribute("aria-expanded", "true");

  fireEvent.keyDown(tree, { key: "ArrowDown" });
  fireEvent.keyDown(tree, { key: "ArrowDown" });
  expect(document.activeElement?.textContent).toContain("utils");
  fireEvent.keyDown(tree, { key: " " });
  await screen.findByText("helper.ts");
  expect(screen.getByRole("treeitem", { name: /^utils$/ })).toHaveAttribute("aria-expanded", "true");

  // ArrowLeft on an expanded dir collapses it without moving
  fireEvent.keyDown(tree, { key: "ArrowLeft" });
  expect(screen.queryByText("helper.ts")).toBeNull();
  expect(document.activeElement?.textContent).toContain("utils");

  // ArrowLeft on a collapsed dir jumps to the parent dir
  fireEvent.keyDown(tree, { key: "ArrowLeft" });
  expect(document.activeElement?.textContent).toContain("src");

  // ArrowLeft on a file jumps to its parent dir
  fireEvent.keyDown(tree, { key: "ArrowDown" });
  expect(document.activeElement?.textContent).toContain("App.tsx");
  fireEvent.keyDown(tree, { key: "ArrowLeft" });
  expect(document.activeElement?.textContent).toContain("src");
});

test("imperative handle moves the selection and refreshes root plus expanded dirs", async () => {
  const { client, request } = fakeClient({ "": NAV_ROOT, src: NAV_CHILDREN });
  const ref = createRef<FileTreeHandle>();
  const { container } = render(<FileTree ref={ref} client={client} sessionId="s1" demo={false} />);
  await screen.findByText("src");
  await waitFor(() => expect(request).toHaveBeenCalledTimes(1));

  act(() => ref.current?.moveBy(1));
  expect(document.activeElement?.textContent).toContain("README.md");
  act(() => ref.current?.moveBy(-1));
  expect(document.activeElement?.textContent).toContain("src");

  fireEvent.keyDown(getTree(container), { key: "Enter" });
  await screen.findByText("App.tsx");

  await act(async () => {
    ref.current?.refresh();
  });
  await waitFor(() => {
    const rootCalls = request.mock.calls.filter(([, params]) => !params || !("path" in params));
    const srcCalls = request.mock.calls.filter(([, params]) => params?.path === "src");
    expect(rootCalls).toHaveLength(2);
    expect(srcCalls).toHaveLength(2);
  });
});
