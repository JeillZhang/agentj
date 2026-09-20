// agentj-ui/apps/tui/src/filesView.test.tsx
import { describe, expect, test } from "bun:test";
import { testRender } from "@opentui/react/test-utils";
import { act } from "react";
import { ThemeProvider } from "./theme.tsx";
import { FilesView } from "./files.tsx";
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
