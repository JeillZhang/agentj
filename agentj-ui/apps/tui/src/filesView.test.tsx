// agentj-ui/apps/tui/src/filesView.test.tsx
import { describe, expect, test } from "bun:test";
import { testRender } from "@opentui/react/test-utils";
import { act, useState } from "react";
import type { RpcClient } from "@agentj/sdk";
import { ThemeProvider } from "./theme.tsx";
import { FilesView, useFileTree } from "./files.tsx";
import { flattenVisible } from "./files.ts";
import type { FileNode } from "./files.ts";

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

describe("useFileTree", () => {
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
