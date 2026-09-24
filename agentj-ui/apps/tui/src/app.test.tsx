import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { testRender } from "@opentui/react/test-utils";
import type { TestRendererSetup } from "@opentui/core/testing";
import { act } from "react";
import { App, getBackoffDelay } from "./app.tsx";
import type { TransportHandle } from "./app.tsx";
import { METHODS, ResponseRouter } from "@agentj/sdk";
import type { TransportStatus } from "@agentj/sdk";
import { MODEL_CONFIG, providersFromStatus, renderModelList } from "./models.tsx";
import { loadFileTreePrefs } from "./filetreePrefs.ts";
import { saveProjects } from "./sidebar.tsx";
import type { RawFileEntry } from "./files.ts";

// Every app render restores/persists TUI state under XDG_CONFIG_HOME; isolate
// each test so consecutive runs stay idempotent (see filesView.test.tsx).
let configDir: string;
let prevXdg: string | undefined;

beforeEach(() => {
  prevXdg = process.env.XDG_CONFIG_HOME;
  configDir = mkdtempSync(join(tmpdir(), "aj-app-"));
  process.env.XDG_CONFIG_HOME = configDir;
});

afterEach(() => {
  if (prevXdg === undefined) delete process.env.XDG_CONFIG_HOME;
  else process.env.XDG_CONFIG_HOME = prevXdg;
  rmSync(configDir, { recursive: true, force: true });
});

function getInput(setup: TestRendererSetup) {
  const input = setup.mockInput as unknown as {
    press?: (k: string) => void;
    type?: (s: string) => Promise<void>;
  };
  if (!input.press) {
    input.press = (k: string) => {
      if (k === "down") setup.mockInput.pressArrow("down");
      else if (k === "up") setup.mockInput.pressArrow("up");
      else if (k === "enter" || k === "return") setup.mockInput.pressEnter();
      else if (k === "esc" || k === "escape") setup.mockInput.pressEscape();
      else if (k === "tab") setup.mockInput.pressTab();
      else setup.mockInput.pressKey(k);
    };
  }
  if (!input.type) {
    input.type = async (s: string) => {
      await setup.mockInput.typeText(s);
    };
  }
  return input as { press: (k: string) => void; type: (s: string) => Promise<void> };
}

function createUnreachable(): TransportHandle {
  const handle: TransportHandle = {
    close() {},
  };
  queueMicrotask(() => act(() => handle.onStatusChange?.("closed")));
  return handle;
}

function createWithAuthError(): TransportHandle {
  return {
    lastError: "403 Forbidden: NotAuthorized",
    close() {},
  };
}

function createStayConnecting(): TransportHandle {
  return {
    close() {},
  };
}

type KernelCall = { method: string } & Record<string, unknown>;

const KERNEL_TREE: Record<string, RawFileEntry[]> = {
  "": [
    { name: "src", path: "src", kind: "dir", git_status: "M" },
    { name: "README.md", path: "README.md", kind: "file", git_status: null },
  ],
  src: [
    { name: "components", path: "src/components", kind: "dir", git_status: null },
    { name: "main.rs", path: "src/main.rs", kind: "file", git_status: "D" },
  ],
  "src/components": [
    { name: "Button.tsx", path: "src/components/Button.tsx", kind: "file", git_status: "A" },
  ],
};

type FakeKernel = {
  calls: KernelCall[];
  drop: (path: string) => void;
  failDepthEight: (times: number) => void;
  deferDepthEight: () => () => void;
  handle: TransportHandle & { send: (line: string) => Promise<string> };
};

function fakeKernel(): FakeKernel {
  const calls: KernelCall[] = [];
  const dropped = new Set<string>();
  let depthEightFailures = 0;
  const depthEightGates: Array<PromiseWithResolvers<void>> = [];
  const live = (path: string): RawFileEntry[] =>
    (KERNEL_TREE[path] ?? []).filter((e) => !dropped.has(e.path));
  const flat = (): RawFileEntry[] => ["", "src", "src/components"].flatMap((p) => live(p));
  const handle = {
    close() {},
    async send(line: string): Promise<string> {
      const req = JSON.parse(line) as { id: number; method: string; params?: Record<string, unknown> };
      const params = req.params ?? {};
      calls.push({ method: req.method, ...params });
      let result: unknown = {};
      if (req.method === METHODS.sessionNew) result = { session_id: "s1" };
      if (req.method === METHODS.fileList) {
        const depth = typeof params.depth === "number" ? params.depth : 1;
        if (depth >= 8) {
          const gate = depthEightGates.shift();
          if (gate) await gate.promise;
          if (depthEightFailures > 0) {
            depthEightFailures -= 1;
            throw new Error("index unavailable");
          }
          result = { entries: flat(), truncated: false };
        } else {
          result = { entries: live(params.path === null ? "" : String(params.path)), truncated: false };
        }
      }
      return JSON.stringify({ jsonrpc: "2.0", id: req.id, result });
    },
  };
  return {
    calls,
    drop: (path) => dropped.add(path),
    failDepthEight: (times) => { depthEightFailures = times; },
    deferDepthEight: () => {
      const gate = Promise.withResolvers<void>();
      depthEightGates.push(gate);
      return () => { gate.resolve(); };
    },
    handle,
  };
}

async function renderApp(
  kernel: FakeKernel,
  options: { width?: number; height?: number } = {},
) {
  const createTransport = (_url: string, _router: ResponseRouter): TransportHandle => {
    queueMicrotask(() => act(() => kernel.handle.onStatusChange?.("open")));
    return kernel.handle;
  };
  const setup = await testRender(
    <App createTransport={createTransport} backoffBaseMs={60000} />,
    options,
  );
  const input = getInput(setup);
  await act(async () => {
    await setup.waitFor(() => setup.captureCharFrame().toLowerCase().includes("connected"));
  });
  return { setup, input };
}

type AppInput = ReturnType<typeof getInput>;

// The terminal parser defers a lone ESC byte until the next input arrives, so
// pressing Esc and typing immediately would deliver Alt+<char> instead; give
// the parser a real tick (same pattern as tui-flow.test.tsx).
async function pressEscape(setup: TestRendererSetup, input: AppInput): Promise<void> {
  await act(async () => {
    input.press("esc");
    await setup.flush();
    await new Promise((resolve) => setTimeout(resolve, 30));
    await setup.flush();
  });
}

// opentui's incremental diff keeps stale glyphs when a text cell shrinks to a
// space, so a same-size resize is used to force a clean repaint before reading
// hint text that changes with the sidebar view.
async function repaint(setup: TestRendererSetup): Promise<void> {
  setup.resize(setup.renderer.width, setup.renderer.height);
  await act(async () => { await setup.flush(); });
}

async function openFilesSidebar(setup: TestRendererSetup, input: AppInput): Promise<void> {
  await act(async () => { input.press("down"); await setup.flush(); });
  await act(async () => { input.press("enter"); await setup.flush(); });
  await act(async () => { await setup.waitFor(() => setup.captureCharFrame().includes("s1")); });
  await act(async () => { input.press("tab"); await setup.flush(); });
  await act(async () => { input.press("f"); await setup.flush(); });
  await act(async () => { await setup.waitFor(() => setup.captureCharFrame().includes("README.md")); });
}

const depthEightCalls = (kernel: FakeKernel): KernelCall[] =>
  kernel.calls.filter((c) => c.method === METHODS.fileList && c.depth === 8);

describe("App connection screen", () => {
  test("unreachable shows kernel unreachable", async () => {
    const setup = await testRender(<App createTransport={createUnreachable} backoffBaseMs={10} />, {});
    await act(async () => {
      await setup.waitFor(() => {
        const frame = setup.captureCharFrame();
        return frame.toLowerCase().includes("kernel unreachable");
      });
    });
    const frame = setup.captureCharFrame();
    expect(frame.toLowerCase()).toContain("kernel unreachable");
    act(() => setup.renderer.destroy());
  });

  test("403 stops reconnect with auth error", async () => {
    let calls = 0;
    const createAuth = (): TransportHandle => {
      calls += 1;
      return createWithAuthError();
    };
    const setup = await testRender(<App createTransport={createAuth} backoffBaseMs={10} />, {});
    await act(async () => {
      await setup.waitFor(() => {
        const frame = setup.captureCharFrame().toLowerCase();
        return frame.includes("auth error") || frame.includes("403");
      });
    });
    const frame = setup.captureCharFrame().toLowerCase();
    expect(frame).toContain("auth");
    {
      const { promise, resolve } = Promise.withResolvers<void>();
      setTimeout(resolve, 50);
      await promise;
    }
    expect(calls).toBe(1);
    expect(frame).not.toContain("kernel unreachable");
    act(() => setup.renderer.destroy());
  });

  test("snapshot contains status line", async () => {
    const setup = await testRender(<App createTransport={createStayConnecting} backoffBaseMs={10000} />, {});
    await act(async () => {
      await setup.flush();
    });
    const frame = setup.captureCharFrame();
    expect(frame.toLowerCase()).toContain("connecting");
    expect(frame.length).toBeGreaterThan(0);
    act(() => setup.renderer.destroy());
  });

  test("exponential backoff capped at 30s", () => {
    expect(getBackoffDelay(0)).toBe(1000);
    expect(getBackoffDelay(1)).toBe(2000);
    expect(getBackoffDelay(4)).toBe(16000);
    expect(getBackoffDelay(5)).toBe(30000);
    expect(getBackoffDelay(10)).toBe(30000);
    expect(getBackoffDelay(6, 500)).toBe(30000);
  });

  test("active session streams standard-frame rows into the transcript", async () => {
    const requests: Array<{ method: string; params: Record<string, unknown> }> = [];
    let router: ResponseRouter | null = null;
    type StreamingHandle = TransportHandle & {
      send: (l: string) => Promise<string>;
      push: (method: string, params: unknown) => void;
    };
    const handle: StreamingHandle = {
      close() {},
      async send(line: string): Promise<string> {
        const req = JSON.parse(line) as { id: number; method: string; params: Record<string, unknown> };
        requests.push({ method: req.method, params: req.params ?? {} });
        const result = req.method === METHODS.sessionNew ? { session_id: "s1" } : {};
        return JSON.stringify({ jsonrpc: "2.0", id: req.id, result });
      },
      push(method: string, params: unknown) {
        router?.feed(`${JSON.stringify({ jsonrpc: "2.0", method, params })}\n`);
      },
    };
    const createStreaming = (url: string, r: ResponseRouter): TransportHandle => {
      router = r;
      queueMicrotask(() => act(() => handle.onStatusChange?.("open")));
      return handle;
    };
    const setup = await testRender(<App createTransport={createStreaming} backoffBaseMs={60000} />, {});
    const input = getInput(setup);
    await act(async () => { await setup.flush(); });
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().toLowerCase().includes("connected"));
    });
    await act(async () => {
      input.press("down");
      await setup.flush();
    });
    await act(async () => {
      input.press("enter");
      await setup.flush();
    });
    await act(async () => { await setup.flush(); });
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("s1"));
    });
    await act(async () => {
      await input.type("hi");
      await setup.flush();
    });
    await act(async () => {
      input.press("enter");
      await setup.flush();
    });
    await act(async () => {
      await setup.waitFor(() => requests.some((r) => r.method === METHODS.sessionRun));
    });
    expect(requests.find((r) => r.method === METHODS.sessionRun)?.params).toEqual({
      session_id: "s1",
      content: "hi",
    });
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("hi"));
    });
    act(() => {
      handle.push(METHODS.progress, {
        session_id: "s1",
        token: 1,
        value: { kind: "text", delta: "hello" },
      });
    });
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("hello"));
    });
    act(() => {
      handle.push(METHODS.progress, {
        session_id: "s1",
        token: 3,
        value: { kind: "usage", usage: { prompt_tokens: 10, completion_tokens: 5 } },
      });
    });
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("in=10 out=5"));
    });
    act(() => setup.renderer.destroy());
  });

  test("kernel provider objects map to ids; absent providers keep the static fallback", () => {
    expect(
      providersFromStatus({
        default_model: "deepseek-chat",
        providers: [
          { id: "deepseek-chat", models: [] },
          { id: "opencode-go", models: [] },
        ],
      }),
    ).toEqual(["deepseek-chat", "opencode-go"]);
    expect(providersFromStatus({ default_model: "deepseek-chat" })).toEqual(
      MODEL_CONFIG.providers,
    );
    expect(providersFromStatus({})).toEqual(MODEL_CONFIG.providers);
    const rendered = renderModelList({
      default_model: "deepseek-chat",
      providers: providersFromStatus({
        providers: [{ id: "opencode-go", models: [] }],
      }),
    });
    expect(rendered).toContain("  opencode-go");
    expect(rendered).not.toContain("[object Object]");
  });
});

test("files view lazy-loads the tree and expands directories", async () => {
  const requests: Array<{ method: string; params: Record<string, unknown> }> = [];
  let router: ResponseRouter | null = null;
  type Handle = TransportHandle & { send: (l: string) => Promise<string> };
  const handle: Handle = {
    close() {},
    async send(line: string): Promise<string> {
      const req = JSON.parse(line) as { id: number; method: string; params: Record<string, unknown> };
      requests.push({ method: req.method, params: req.params ?? {} });
      let result: unknown = {};
      if (req.method === METHODS.sessionNew) result = { session_id: "s1" };
      if (req.method === METHODS.fileList) {
        result = req.params?.path
          ? { entries: [{ name: "main.rs", path: "src/main.rs", kind: "file", git_status: "D" }], truncated: false }
          : { entries: [
              { name: "src", path: "src", kind: "dir", git_status: "M" },
              { name: "README.md", path: "README.md", kind: "file", git_status: null },
            ], truncated: false };
      }
      return JSON.stringify({ jsonrpc: "2.0", id: req.id, result });
    },
  };
  const createHandle = (_url: string, r: ResponseRouter): TransportHandle => {
    router = r;
    queueMicrotask(() => act(() => handle.onStatusChange?.("open")));
    return handle;
  };
  const setup = await testRender(<App createTransport={createHandle} backoffBaseMs={60000} />, {});
  const input = getInput(setup);
  await act(async () => { await setup.waitFor(() => setup.captureCharFrame().toLowerCase().includes("connected")); });
  // Tab enters sidebar focus (composer mode keeps letters in the composer);
  // with no active session yet, f opens Files with a placeholder and fires no request
  await act(async () => { input.press("tab"); await setup.flush(); });
  await act(async () => { input.press("f"); await setup.flush(); });
  await act(async () => { await setup.waitFor(() => setup.captureCharFrame().includes("select a session")); });
  expect(requests.some((r) => r.method === METHODS.fileList)).toBe(false);
  await act(async () => { input.press("f"); await setup.flush(); });
  await act(async () => { input.press("down"); await setup.flush(); });
  await act(async () => { input.press("enter"); await setup.flush(); });   // creates s1
  await act(async () => { await setup.waitFor(() => setup.captureCharFrame().includes("s1")); });
  await act(async () => { input.press("f"); await setup.flush(); });
  await act(async () => {
    await setup.waitFor(() => requests.some((r) => r.method === METHODS.fileList));
  });
  expect(requests.find((r) => r.method === METHODS.fileList)?.params).toEqual({
    session_id: "s1", path: null, depth: 1,
  });
  await act(async () => { await setup.waitFor(() => setup.captureCharFrame().includes("src")); });
  await act(async () => { input.press("enter"); await setup.flush(); });   // expand src
  await act(async () => {
    await setup.waitFor(() => requests.some((r) => r.method === METHODS.fileList && r.params.path === "src"));
  });
  await act(async () => { await setup.waitFor(() => setup.captureCharFrame().includes("main.rs")); });
  // j selects the child row; h jumps back up to the parent dir row
  await act(async () => { input.press("j"); await setup.flush(); });
  await act(async () => { await setup.waitFor(() => /●\s+main\.rs/.test(setup.captureCharFrame())); });
  await act(async () => { input.press("h"); await setup.flush(); });
  await act(async () => { await setup.waitFor(() => /●\s*▼\s*src/.test(setup.captureCharFrame())); });
  // h on the expanded dir collapses it; l re-expands from the cached children (no new request)
  const beforeCollapse = requests.filter((r) => r.method === METHODS.fileList).length;
  await act(async () => { input.press("h"); await setup.flush(); });
  await act(async () => { await setup.waitFor(() => !setup.captureCharFrame().includes("main.rs")); });
  await act(async () => { input.press("l"); await setup.flush(); });
  await act(async () => { await setup.waitFor(() => setup.captureCharFrame().includes("main.rs")); });
  expect(requests.filter((r) => r.method === METHODS.fileList).length).toBe(beforeCollapse);
  // r refreshes the root and re-fetches expanded dirs
  const beforeRefresh = requests.filter((r) => r.method === METHODS.fileList).length;
  await act(async () => { input.press("r"); await setup.flush(); });
  await act(async () => {
    await setup.waitFor(() =>
      requests.filter((r) => r.method === METHODS.fileList).length >= beforeRefresh + 2);
  });
  const refreshed = requests.filter((r) => r.method === METHODS.fileList).slice(beforeRefresh);
  expect(refreshed.some((r) => r.params.path === null)).toBe(true);
  expect(refreshed.some((r) => r.params.path === "src")).toBe(true);
  await act(async () => { input.press("f"); await setup.flush(); });
  await act(async () => { await setup.waitFor(() => setup.captureCharFrame().includes("Threads")); });
  act(() => setup.renderer.destroy());
  void router;
});

test("BUG-009: typing in the composer never triggers sidebar/file shortcuts", async () => {
  const requests: Array<{ method: string; params: Record<string, unknown> }> = [];
  let router: ResponseRouter | null = null;
  type Handle = TransportHandle & { send: (l: string) => Promise<string> };
  const handle: Handle = {
    close() {},
    async send(line: string): Promise<string> {
      const req = JSON.parse(line) as { id: number; method: string; params: Record<string, unknown> };
      requests.push({ method: req.method, params: req.params ?? {} });
      let result: unknown = {};
      if (req.method === METHODS.sessionNew) result = { session_id: "s1" };
      if (req.method === METHODS.fileList) {
        result = { entries: [{ name: "src", path: "src", kind: "dir", git_status: "M" }], truncated: false };
      }
      return JSON.stringify({ jsonrpc: "2.0", id: req.id, result });
    },
  };
  const createHandle = (_url: string, r: ResponseRouter): TransportHandle => {
    router = r;
    queueMicrotask(() => act(() => handle.onStatusChange?.("open")));
    return handle;
  };
  const setup = await testRender(<App createTransport={createHandle} backoffBaseMs={60000} />, {});
  const input = getInput(setup);
  await act(async () => { await setup.waitFor(() => setup.captureCharFrame().toLowerCase().includes("connected")); });
  // create an active session so the composer is visible (SessionList: down + enter)
  await act(async () => { input.press("down"); await setup.flush(); });
  await act(async () => { input.press("enter"); await setup.flush(); });
  await act(async () => { await setup.waitFor(() => setup.captureCharFrame().includes("s1")); });
  expect(setup.captureCharFrame()).toContain("Threads");
  // typing "of" (contains f) must not toggle the sidebar to Files
  await act(async () => { await input.type("of"); await setup.flush(); });
  let frame = setup.captureCharFrame();
  expect(frame).toContain("Threads");
  expect(frame).not.toContain("Files");
  expect(frame).toContain("of");
  expect(requests.filter((r) => r.method === METHODS.fileList)).toHaveLength(0);
  // typing " for" (f would toggle, r would refresh) must not switch views or fetch files
  await act(async () => { await input.type(" for"); await setup.flush(); });
  frame = setup.captureCharFrame();
  expect(frame).toContain("Threads");
  expect(frame).not.toContain("Files");
  expect(frame).toContain("of for");
  expect(requests.filter((r) => r.method === METHODS.fileList)).toHaveLength(0);
  // typing "need" (contains n) must not create another session
  await act(async () => { await input.type(" need"); await setup.flush(); });
  expect(requests.filter((r) => r.method === METHODS.sessionNew)).toHaveLength(1);
  // Enter submits the full composer text unchanged
  await act(async () => { input.press("enter"); await setup.flush(); });
  await act(async () => {
    await setup.waitFor(() => requests.some((r) => r.method === METHODS.sessionRun));
  });
  expect(requests.find((r) => r.method === METHODS.sessionRun)?.params).toEqual({
    session_id: "s1",
    content: "of for need",
  });
  act(() => setup.renderer.destroy());
  void router;
});

describe("files search mode", () => {
  test("s enters search, printables filter, r rebuilds, enter jumps", async () => {
    const kernel = fakeKernel();
    const { setup, input } = await renderApp(kernel);
    await openFilesSidebar(setup, input);
    await act(async () => { input.press("s"); await setup.flush(); });
    expect(depthEightCalls(kernel)).toHaveLength(1);
    expect(depthEightCalls(kernel)[0]).toMatchObject({ path: null, depth: 8 });
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("type filter"));
    });
    await act(async () => { await input.type("fnc"); await setup.flush(); });
    expect(depthEightCalls(kernel)).toHaveLength(1); // f/n/c stay query text
    expect(kernel.calls.filter((c) => c.method === METHODS.sessionNew)).toHaveLength(1);
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("type filter"));
    });
    expect(setup.captureCharFrame()).toContain("Files");
    expect(setup.captureCharFrame()).not.toContain("Threads");
    await pressEscape(setup, input); // clears the query
    await act(async () => { await input.type("main"); await setup.flush(); });
    await act(async () => { input.press("r"); await setup.flush(); }); // rebuilds the index
    expect(depthEightCalls(kernel)).toHaveLength(2);
    const callsBeforeJump = kernel.calls.filter((c) => c.method === METHODS.fileList).length;
    await act(async () => { input.press("enter"); await setup.flush(); }); // jump
    await act(async () => {
      await setup.waitFor(() => !setup.captureCharFrame().includes("type filter"));
    });
    await act(async () => { await setup.waitFor(() => /●\s+main\.rs/.test(setup.captureCharFrame())); });
    expect(kernel.calls).toContainEqual(
      expect.objectContaining({ method: METHODS.fileList, path: "src", depth: 1 }),
    );
    expect(kernel.calls.filter((c) => c.method === METHODS.fileList).length).toBeGreaterThan(callsBeforeJump);
    expect(setup.captureCharFrame()).not.toContain("type filter");
    act(() => setup.renderer.destroy());
  });

  test("enter on a directory result expands and selects it", async () => {
    const kernel = fakeKernel();
    const { setup, input } = await renderApp(kernel);
    await openFilesSidebar(setup, input);
    await act(async () => { input.press("s"); await setup.flush(); });
    await act(async () => { await input.type("components"); await setup.flush(); });
    await act(async () => { input.press("enter"); await setup.flush(); });
    await act(async () => {
      await setup.waitFor(() => !setup.captureCharFrame().includes("type filter"));
    });
    await act(async () => { await setup.waitFor(() => /●\s*▼\s*components/.test(setup.captureCharFrame())); });
    await act(async () => { await setup.waitFor(() => setup.captureCharFrame().includes("Button.tsx")); });
    expect(kernel.calls).toContainEqual(
      expect.objectContaining({ method: METHODS.fileList, path: "src/components", depth: 1 }),
    );
    act(() => setup.renderer.destroy());
  });

  test("esc clears a non-empty query before it exits search", async () => {
    const kernel = fakeKernel();
    const { setup, input } = await renderApp(kernel);
    await openFilesSidebar(setup, input);
    await act(async () => { input.press("s"); await setup.flush(); });
    await act(async () => { await input.type("ab"); await setup.flush(); });
    const before = depthEightCalls(kernel).length;
    await pressEscape(setup, input);
    // f and r would toggle the view / refresh the tree unless search stayed active
    await act(async () => { input.press("f"); await setup.flush(); });
    await act(async () => { input.press("r"); await setup.flush(); });
    await act(async () => {
      await setup.waitFor(() => depthEightCalls(kernel).length > before);
    });
    expect(setup.captureCharFrame()).not.toContain("Threads");
    await pressEscape(setup, input); // clears "f"
    await pressEscape(setup, input); // now exits
    await act(async () => {
      await setup.waitFor(() => !setup.captureCharFrame().includes("type filter"));
    });
    await act(async () => { input.press("f"); await setup.flush(); });
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("Threads"));
    });
    act(() => setup.renderer.destroy());
  });

  test("r retries the index after a failed search request", async () => {
    const kernel = fakeKernel();
    const { setup, input } = await renderApp(kernel);
    await openFilesSidebar(setup, input);
    kernel.failDepthEight(1);
    await act(async () => { input.press("s"); await setup.flush(); });
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("index unavailable"));
    });
    expect(depthEightCalls(kernel)).toHaveLength(1);
    await act(async () => { await input.type("main"); await setup.flush(); });
    await act(async () => { input.press("r"); await setup.flush(); });
    await act(async () => {
      await setup.waitFor(() => !setup.captureCharFrame().includes("index unavailable"));
    });
    expect(depthEightCalls(kernel)).toHaveLength(2);
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("main.rs"));
    });
    act(() => setup.renderer.destroy());
  });

  test("keeps the query typed while the index request is in flight", async () => {
    const kernel = fakeKernel();
    const { setup, input } = await renderApp(kernel);
    await openFilesSidebar(setup, input);
    const release = kernel.deferDepthEight();
    await act(async () => { input.press("s"); await setup.flush(); });
    await act(async () => {
      await setup.waitFor(() => depthEightCalls(kernel).length === 1);
    });
    // the depth-8 reply is unresolved: every keystroke must stay in the query
    await act(async () => { await input.type("main"); await setup.flush(); });
    await act(async () => { release(); await setup.flush(); });
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("main.rs"));
    });
    expect(depthEightCalls(kernel)).toHaveLength(1);
    expect(setup.captureCharFrame()).not.toContain("README.md");
    act(() => setup.renderer.destroy());
  });

  test("keeps the failed-index error while typing and clears it only after r", async () => {
    const kernel = fakeKernel();
    const { setup, input } = await renderApp(kernel);
    await openFilesSidebar(setup, input);
    kernel.failDepthEight(1);
    await act(async () => { input.press("s"); await setup.flush(); });
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("index unavailable"));
    });
    await act(async () => { await input.type("main"); await setup.flush(); });
    await repaint(setup);
    // C2: keystrokes must not hide the failure; only a successful r clears it
    expect(setup.captureCharFrame()).toContain("index unavailable");
    await act(async () => { input.press("r"); await setup.flush(); });
    await act(async () => {
      await setup.waitFor(() => !setup.captureCharFrame().includes("index unavailable"));
    });
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("main.rs"));
    });
    act(() => setup.renderer.destroy());
  });

  test("jump loads ancestors depth-first and selects the nested target", async () => {
    const kernel = fakeKernel();
    const { setup, input } = await renderApp(kernel);
    await openFilesSidebar(setup, input);
    await act(async () => { input.press("s"); await setup.flush(); });
    await act(async () => { await input.type("Button"); await setup.flush(); });
    await act(async () => { input.press("enter"); await setup.flush(); });
    await act(async () => {
      await setup.waitFor(() => /●\s+Button\.tsx/.test(setup.captureCharFrame()));
    });
    const treeCalls = kernel.calls
      .filter((c) => c.method === METHODS.fileList && c.depth === 1)
      .map((c) => c.path);
    expect(treeCalls).toEqual([null, "src", "src/components"]);
    expect(setup.captureCharFrame()).not.toContain("type filter");
    act(() => setup.renderer.destroy());
  });

  test("a stale search target exits, refreshes the root and reports once", async () => {
    const kernel = fakeKernel();
    const { setup, input } = await renderApp(kernel);
    await openFilesSidebar(setup, input);
    await act(async () => { input.press("s"); await setup.flush(); }); // index snapshot has main.rs
    await act(async () => { await input.type("main"); await setup.flush(); });
    kernel.drop("src/main.rs"); // deleted after indexing: reveal must notice
    const rootCalls = () =>
      kernel.calls.filter((c) => c.method === METHODS.fileList && c.depth === 1 && c.path === null).length;
    const before = rootCalls();
    await act(async () => { input.press("enter"); await setup.flush(); });
    await act(async () => { await setup.waitFor(() => rootCalls() > before); });
    await act(async () => {
      await setup.waitFor(() => !setup.captureCharFrame().includes("type filter"));
    });
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("path no longer exists"));
    });
    expect(setup.captureCharFrame()).toContain("path no longer exists");
    act(() => setup.renderer.destroy());
  });

  test("enter on an empty result set keeps search mode open", async () => {
    const kernel = fakeKernel();
    const { setup, input } = await renderApp(kernel);
    await openFilesSidebar(setup, input);
    await act(async () => { input.press("s"); await setup.flush(); });
    await act(async () => { await input.type("zzz"); await setup.flush(); });
    await act(async () => { input.press("enter"); await setup.flush(); });
    expect(setup.captureCharFrame()).toContain("type filter");
    act(() => setup.renderer.destroy());
  });

  test("sidebar advertises the search hint while searching and restores it after", async () => {
    const kernel = fakeKernel();
    const { setup, input } = await renderApp(kernel);
    await openFilesSidebar(setup, input);
    await repaint(setup);
    // Files view with sidebar focus advertises the normal shortcut hint
    expect(setup.captureCharFrame()).toContain("s search");
    await act(async () => { input.press("s"); await setup.flush(); });
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("esc back"));
    });
    await repaint(setup);
    // search mode must replace the shortcut hint in the sidebar header
    expect(setup.captureCharFrame()).toContain("esc back");
    expect(setup.captureCharFrame()).not.toContain("s search");
    await pressEscape(setup, input); // empty query exits search
    await repaint(setup);
    expect(setup.captureCharFrame()).toContain("s search");
    expect(setup.captureCharFrame()).not.toContain("esc back");
    act(() => setup.renderer.destroy());
  });

  test("persists expansion under the active session's project group", async () => {
    saveProjects([{ id: "p1", name: "Foo" }]);
    const kernel = fakeKernel();
    const { setup, input } = await renderApp(kernel);
    await act(async () => { input.press("down"); await setup.flush(); });
    await act(async () => { input.press("enter"); await setup.flush(); });
    await act(async () => { await setup.waitFor(() => setup.captureCharFrame().includes("s1")); });
    await act(async () => { input.press("tab"); await setup.flush(); });
    await act(async () => { input.press("f"); await setup.flush(); });
    await act(async () => { await setup.waitFor(() => setup.captureCharFrame().includes("README.md")); });
    await act(async () => { input.press("enter"); await setup.flush(); });
    await act(async () => { await setup.waitFor(() => setup.captureCharFrame().includes("main.rs")); });
    expect(loadFileTreePrefs().expanded.p1).toEqual(["src"]);
    act(() => setup.renderer.destroy());
  });
});
