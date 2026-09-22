// agentj-ui/apps/tui/src/filesView.test.tsx
import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { testRender } from "@opentui/react/test-utils";
import type { TestRendererSetup } from "@opentui/core/testing";
import { act, useState } from "react";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { RpcClient } from "@agentj/sdk";
import { ThemeProvider } from "./theme.tsx";
import { FilesView, useFileTree } from "./files.tsx";
import { flattenVisible } from "./files.ts";
import type { FileNode, RawFileEntry, SearchEntry } from "./files.ts";
import { loadFileTreePrefs, saveFileTreePrefs } from "./filetreePrefs.ts";

function node(partial: Partial<FileNode> & { name: string; path: string; kind: "file" | "dir" }): FileNode {
  return {
    gitStatus: null, expanded: false, loading: false, error: null,
    truncated: false, children: null, ...partial,
  };
}

async function renderView(props: Parameters<typeof FilesView>[0]) {
  const setup = await testRender(
    <ThemeProvider><FilesView {...props} /></ThemeProvider>, { width: 60, height: 20 },
  );
  await act(async () => { await setup.flush(); });
  return setup;
}

describe("FilesView", () => {
  test("renders dir rows with status letters and expanded children", async () => {
    const setup = await renderView({
      nodes: [node({ name: "src", path: "src", kind: "dir", gitStatus: "M", expanded: true,
        children: [node({ name: "main.rs", path: "src/main.rs", kind: "file", gitStatus: "D" })] })],
      selected: 0, error: null, truncated: false, noSession: false, width: 28,
    });
    const frame = setup.captureCharFrame();
    expect(frame).toContain("src");
    expect(frame).toContain("main.rs");
    expect(frame).toContain("M");
    expect(frame).toContain("D");
    act(() => setup.renderer.destroy());
  });

  test("placeholders for no session, loading, empty, error and truncation", async () => {
    const base = { selected: 0, error: null, truncated: false, width: 28 };
    const noSession = await renderView({ nodes: null, noSession: true, ...base });
    expect(noSession.captureCharFrame()).toContain("select a session");
    act(() => noSession.renderer.destroy());

    const loading = await renderView({ nodes: null, noSession: false, ...base });
    expect(loading.captureCharFrame()).toContain("loading");
    act(() => loading.renderer.destroy());

    const empty = await renderView({ nodes: [], noSession: false, ...base });
    expect(empty.captureCharFrame()).toContain("(empty)");
    act(() => empty.renderer.destroy());

    const errored = await renderView({
      nodes: [node({ name: "src", path: "src", kind: "dir" })],
      selected: 0, error: "boom", truncated: false, noSession: false, width: 28,
    });
    const erroredFrame = errored.captureCharFrame();
    expect(erroredFrame).toContain("boom");
    expect(erroredFrame).toContain("src");
    act(() => errored.renderer.destroy());

    const cut = await renderView({
      nodes: [node({ name: "src", path: "src", kind: "dir" })],
      selected: 0, error: null, truncated: true, noSession: false, width: 28,
    });
    expect(cut.captureCharFrame()).toContain("truncated");
    act(() => cut.renderer.destroy());

    const perDir = await renderView({
      nodes: [node({ name: "src", path: "src", kind: "dir", expanded: true, truncated: true,
        children: [node({ name: "main.rs", path: "src/main.rs", kind: "file" })] })],
      selected: 0, error: null, truncated: false, noSession: false, width: 28,
    });
    expect(perDir.captureCharFrame()).toContain("truncated");
    act(() => perDir.renderer.destroy());

    const emptyDir = await renderView({
      nodes: [node({ name: "empty", path: "empty", kind: "dir", expanded: true, children: [] })],
      selected: 0, error: null, truncated: false, noSession: false, width: 28,
    });
    expect(emptyDir.captureCharFrame()).toContain("(empty)");
    act(() => emptyDir.renderer.destroy());

    const failedDir = await renderView({
      nodes: [node({ name: "gone", path: "gone", kind: "dir", expanded: true,
        error: "PathNotAllowed", children: null })],
      selected: 0, error: null, truncated: false, noSession: false, width: 28,
    });
    const failedFrame = failedDir.captureCharFrame();
    expect(failedFrame).toContain("PathNotAllowed");
    expect(failedFrame).toContain("gone");
    act(() => failedDir.renderer.destroy());
  });
});

describe("FilesView search mode", () => {
  const TWO_ROWS: SearchEntry[] = [
    { name: "main.rs", path: "src/main.rs", kind: "file", gitStatus: "M" },
    { name: "main.md", path: "docs/main.md", kind: "file", gitStatus: null },
  ];

  test("renders matching rows with parent dirs, letters and the selected marker", async () => {
    const setup = await renderView({
      nodes: null, selected: 1, error: null, truncated: false, noSession: false, width: 28,
      search: { active: true, query: "ma", rows: TWO_ROWS, total: 2, truncated: true, error: null },
    });
    const frame = setup.captureCharFrame();
    const lines = frame.split("\n");
    expect(frame).toContain("type filter · enter jump · esc back · partial");
    expect(frame).toContain("main.rs");
    expect(frame).toContain("src/");
    expect(frame).toContain("main.md");
    expect(frame).toContain("docs/");
    const unselected = lines.find((l) => l.includes("main.rs"))!;
    const selected = lines.find((l) => l.includes("main.md"))!;
    expect(unselected).not.toContain("●");
    expect(selected).toContain("●");
    expect(unselected).toContain("M");
    act(() => setup.renderer.destroy());
  });

  test("renders an inline error row while the hint line stays visible", async () => {
    const setup = await renderView({
      nodes: null, selected: 0, error: null, truncated: false, noSession: false, width: 28,
      search: { active: true, query: "ma", rows: [], total: 0, truncated: false, error: "file/list failed" },
    });
    const frame = setup.captureCharFrame();
    expect(frame).toContain("file/list failed");
    expect(frame).toContain("esc back");
    expect(frame).not.toContain("partial");
    act(() => setup.renderer.destroy());
  });

  test("keeps the tree path when search is absent or inactive", async () => {
    const setup = await renderView({
      nodes: [node({ name: "src", path: "src", kind: "dir", gitStatus: "M" })],
      selected: 0, error: null, truncated: false, noSession: false, width: 28,
      search: { active: false, query: "", rows: [], total: 0, truncated: false, error: null },
    });
    const frame = setup.captureCharFrame();
    expect(frame).toContain("src");
    expect(frame).not.toContain("type filter");
    act(() => setup.renderer.destroy());
  });

  test("scrolls the results so a distant selection stays visible", async () => {
    const control = { setSelected: (_: number) => {} };
    const setup = await testRender(
      <ThemeProvider><SearchScrollProbe control={control} /></ThemeProvider>, { width: 60, height: 12 },
    );
    await act(async () => { await setup.flush(); });
    expect(setup.captureCharFrame()).toContain("file00");
    act(() => control.setSelected(30));
    await act(async () => { await setup.flush(); });
    const frame = setup.captureCharFrame();
    expect(frame).toContain("file30");
    expect(frame).not.toContain("file00");
    act(() => setup.renderer.destroy());
  });
});

function SearchScrollProbe({ control }: { control: { setSelected: (n: number) => void } }) {
  const [selected, setSelected] = useState(0);
  control.setSelected = setSelected;
  const rows: SearchEntry[] = Array.from({ length: 40 }, (_, i) => ({
    name: `file${String(i).padStart(2, "0")}.ts`,
    path: `src/file${String(i).padStart(2, "0")}.ts`,
    kind: "file",
    gitStatus: null,
  }));
  return (
    <FilesView
      nodes={null} selected={selected} error={null} truncated={false} noSession={false} width={28}
      search={{ active: true, query: "file", rows, total: rows.length, truncated: false, error: null }}
    />
  );
}

type Deferred = { promise: Promise<unknown>; resolve: (value: unknown) => void };

function deferred(): Deferred {
  let resolve!: (value: unknown) => void;
  const promise = new Promise<unknown>((res) => { resolve = res; });
  return { promise, resolve };
}

type FileListCall = { session: unknown; deferred: Deferred };

function fakeClient(calls: FileListCall[]): RpcClient {
  return {
    request: (_method: string, params: Record<string, unknown>) => {
      const pending = deferred();
      calls.push({ session: params.session_id, deferred: pending });
      return pending.promise;
    },
  } as unknown as RpcClient;
}

function FilesProbe({ client, control }: {
  client: RpcClient | null;
  control: { setSession: (sessionId: string | null) => void };
}) {
  const [sessionId, setSessionId] = useState<string | null>("s1");
  control.setSession = setSessionId;
  const tree = useFileTree(client, sessionId);
  const rows = flattenVisible(tree.nodes ?? []);
  return (
    <box flexDirection="column">
      {rows.map((row) => <text key={row.node.path}>{row.node.name}</text>)}
    </box>
  );
}

type TreeCall = { path: string | null; depth: number };

type PendingReply = { path: string | null; resolve: (value: unknown) => void };

function treeReply(tree: Record<string, string[]>, path: string | null) {
  const entries: RawFileEntry[] = path === null
    ? Object.keys(tree)
        .filter((p) => !p.includes("/"))
        .map((p) => ({ name: p, path: p, kind: "dir" as const }))
    : (tree[path] ?? []).map((name) => ({ name, path: `${path}/${name}`, kind: "dir" as const }));
  return { entries, truncated: false };
}

function manualTreeClient(tree: Record<string, string[]>) {
  const calls: TreeCall[] = [];
  const pending: PendingReply[] = [];
  const client = {
    request: (_method: string, params: Record<string, unknown>) =>
      new Promise((resolve) => {
        const path = (params.path as string | null | undefined) ?? null;
        calls.push({ path, depth: params.depth as number });
        pending.push({ path, resolve });
      }),
  } as unknown as RpcClient;
  const settle = async (setup: TestRendererSetup): Promise<void> => {
    await act(async () => { await setup.flush(); });
    for (let i = 0; i < 25 && pending.length > 0; i += 1) {
      const call = pending.shift()!;
      await act(async () => { call.resolve(treeReply(tree, call.path)); });
      await act(async () => { await setup.flush(); });
    }
  };
  return { client, calls, settle };
}

type TreeControl = {
  toggle: (node: FileNode) => void;
  collapse: (path: string) => void;
  find: (path: string) => FileNode | null;
};

function TreeProbe({ client, sessionId, projectKey, control }: {
  client: RpcClient;
  sessionId: string;
  projectKey: string;
  control?: TreeControl;
}) {
  const tree = useFileTree(client, sessionId, projectKey);
  if (control) {
    control.toggle = tree.toggle;
    control.collapse = tree.collapse;
    control.find = (path) =>
      flattenVisible(tree.nodes ?? []).find((row) => row.node.path === path)?.node ?? null;
  }
  const rows = flattenVisible(tree.nodes ?? []);
  return (
    <box flexDirection="column">
      {rows.map((row) => (
        <text key={`${row.node.path}#${row.note ? "note" : "row"}`}>
          {row.note ? row.note.text : row.node.path}
        </text>
      ))}
    </box>
  );
}

function renderTree(client: RpcClient, control?: TreeControl) {
  return testRender(
    <ThemeProvider><TreeProbe client={client} sessionId="s1" projectKey="p1" control={control} /></ThemeProvider>,
    { width: 40, height: 12 },
  );
}

const TREE: Record<string, string[]> = { src: ["components"], "src/components": [] };

describe("useFileTree", () => {
  let configDir: string;
  let prevXdg: string | undefined;

  beforeEach(() => {
    prevXdg = process.env.XDG_CONFIG_HOME;
    configDir = mkdtempSync(join(tmpdir(), "aj-filetree-view-"));
    process.env.XDG_CONFIG_HOME = configDir;
  });

  afterEach(() => {
    if (prevXdg === undefined) delete process.env.XDG_CONFIG_HOME;
    else process.env.XDG_CONFIG_HOME = prevXdg;
    rmSync(configDir, { recursive: true, force: true });
  });

  test("restores persisted expanded paths depth-ascending after the root load", async () => {
    saveFileTreePrefs({ expanded: { p1: ["src", "src/components"] } }, ["p1"]);
    const { client, calls, settle } = manualTreeClient(TREE);
    const setup = await renderTree(client);
    await settle(setup);
    expect(calls.map((c) => c.path)).toEqual([null, "src", "src/components"]);
    expect(calls.filter((c) => c.path === "src")).toHaveLength(1);
    const frame = setup.captureCharFrame();
    expect(frame).toContain("src");
    expect(frame).toContain("src/components");
    act(() => setup.renderer.destroy());
  });

  test("skips a persisted path that no longer exists and still restores the rest", async () => {
    saveFileTreePrefs({ expanded: { p1: ["src", "gone/deep", "src/components"] } }, ["p1"]);
    const { client, calls, settle } = manualTreeClient(TREE);
    const setup = await renderTree(client);
    await settle(setup);
    expect(calls.some((c) => c.path === "gone" || c.path === "gone/deep")).toBe(false);
    const frame = setup.captureCharFrame();
    expect(frame).toContain("src/components");
    expect(frame).not.toContain("error");
    act(() => setup.renderer.destroy());
  });

  test("persists the live expanded set on toggle and collapse", async () => {
    const { client, settle } = manualTreeClient(TREE);
    const control: TreeControl = { toggle: () => {}, collapse: () => {}, find: () => null };
    const setup = await renderTree(client, control);
    await settle(setup);

    act(() => control.toggle(control.find("src")!));
    expect(loadFileTreePrefs().expanded.p1).toEqual(["src"]);
    await settle(setup);

    act(() => control.toggle(control.find("src/components")!));
    expect(loadFileTreePrefs().expanded.p1).toEqual(["src", "src/components"]);

    act(() => control.collapse("src"));
    expect(loadFileTreePrefs().expanded.p1).toEqual([]);
    act(() => setup.renderer.destroy());
  });

  test("drops a file-list reply that resolves after the session changed", async () => {
    const calls: FileListCall[] = [];
    const control = { setSession: (_: string | null) => {} };
    const setup = await testRender(
      <ThemeProvider><FilesProbe client={fakeClient(calls)} control={control} /></ThemeProvider>,
      { width: 40, height: 10 },
    );
    await act(async () => { await setup.flush(); });
    expect(calls).toHaveLength(1);
    expect(calls[0]!.session).toBe("s1");

    act(() => control.setSession("s2"));
    await act(async () => { await setup.flush(); });
    expect(calls).toHaveLength(2);
    expect(calls[1]!.session).toBe("s2");

    // s1's late reply must never patch s2's tree.
    await act(async () => {
      calls[0]!.deferred.resolve({ entries: [{ name: "s1-only.txt", path: "s1-only.txt", kind: "file" }] });
    });
    await act(async () => { await setup.flush(); });
    expect(setup.captureCharFrame()).not.toContain("s1-only.txt");

    // s2's own reply still lands normally.
    await act(async () => {
      calls[1]!.deferred.resolve({ entries: [{ name: "s2-only.txt", path: "s2-only.txt", kind: "file" }] });
    });
    await act(async () => { await setup.flush(); });
    expect(setup.captureCharFrame()).toContain("s2-only.txt");
    act(() => setup.renderer.destroy());
  });
});
