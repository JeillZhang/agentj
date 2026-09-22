import { describe, expect, test } from "bun:test";
import { testRender } from "@opentui/react/test-utils";
import { act } from "react";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { ThemeProvider } from "./theme.tsx";

describe("Sidebar", () => {
  test("renders Threads and projects", async () => {
    const { default: Sidebar } = await import("./sidebar.tsx");
    const setup = await testRender(<Sidebar projects={[{ id: "foo", name: "Foo" }]} sessions={[{ id: "a", preview: "hi" }]} />, {});
    await act(async () => {
      await setup.flush();
    });
    const frame = setup.captureCharFrame();
    expect(frame).toMatch(/Threads/);
    expect(frame).toMatch(/Foo/);
    expect(frame).toMatch(/Settings/);
    expect(frame).toMatch(/Help/);
    act(() => setup.renderer.destroy());
  });

  test("sidebar width constant is 32 chars", async () => {
    const mod = await import("./sidebar.tsx");
    expect(mod.SIDEBAR_WIDTH).toBe(32);
    const { default: Sidebar } = mod;
    const setup = await testRender(<Sidebar projects={[]} sessions={[]} width={32} />, { width: 80, height: 24 });
    await act(async () => {
      await setup.flush();
    });
    const frame = setup.captureCharFrame();
    // sidebar should be present and not crash; verify constant
    expect(mod.SIDEBAR_WIDTH).toBe(32);
    // frame should contain Threads header even with empty projects
    expect(frame).toMatch(/Threads/);
    act(() => setup.renderer.destroy());
  });

  test("shows ▶ collapsed and ▼ expanded for projects", async () => {
    const { default: Sidebar } = await import("./sidebar.tsx");
    const setupCollapsed = await testRender(
      <Sidebar projects={[{ id: "foo", name: "Foo" }]} sessions={[{ id: "a", preview: "hi" }]} collapsed={{ foo: true }} />,
      {},
    );
    await act(async () => {
      await setupCollapsed.flush();
    });
    let frame = setupCollapsed.captureCharFrame();
    expect(frame).toMatch(/▶/);
    act(() => setupCollapsed.renderer.destroy());

    const setupExpanded = await testRender(
      <Sidebar projects={[{ id: "bar", name: "Bar" }]} sessions={[{ id: "a", preview: "hi" }]} collapsed={{ bar: false }} />,
      {},
    );
    await act(async () => {
      await setupExpanded.flush();
    });
    frame = setupExpanded.captureCharFrame();
    expect(frame).toMatch(/▼/);
    act(() => setupExpanded.renderer.destroy());
  });

  test("collapsed state persists to projects.json via helpers", async () => {
    const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "agentj-sidebar-test-"));
    const prevXdg = process.env.XDG_CONFIG_HOME;
    process.env.XDG_CONFIG_HOME = tmp;
    try {
      const mod = await import("./sidebar.tsx");
      // ensure clean
      const p = mod.getProjectsPath ? mod.getProjectsPath() : path.join(tmp, "agentj", "projects.json");
      // initial load should be empty
      const initial = mod.loadCollapsed ? mod.loadCollapsed() : {};
      expect(initial).toEqual({});

      if (mod.saveCollapsed) {
        mod.saveCollapsed({ foo: true });
        const loaded = mod.loadCollapsed();
        expect(loaded).toEqual({ foo: true });
        // verify file exists at expected path
        expect(fs.existsSync(p)).toBe(true);
        const raw = fs.readFileSync(p, "utf8");
        expect(raw).toMatch(/foo/);
        // toggle
        mod.saveCollapsed({ foo: false, bar: true });
        expect(mod.loadCollapsed()).toEqual({ foo: false, bar: true });
      }
    } finally {
      if (prevXdg === undefined) delete process.env.XDG_CONFIG_HOME;
      else process.env.XDG_CONFIG_HOME = prevXdg;
      fs.rmSync(tmp, { recursive: true, force: true });
    }
  });

  test("footer shows Settings and Help", async () => {
    const { default: Sidebar } = await import("./sidebar.tsx");
    const setup = await testRender(<Sidebar projects={[]} sessions={[]} />, {});
    await act(async () => {
      await setup.flush();
    });
    const frame = setup.captureCharFrame();
    expect(frame).toMatch(/Settings/);
    expect(frame).toMatch(/Help/);
    act(() => setup.renderer.destroy());
  });

  test("sessions indented with active marker ●", async () => {
    const { default: Sidebar } = await import("./sidebar.tsx");
    const setup = await testRender(
      <Sidebar
        projects={[{ id: "foo", name: "Foo" }]}
        sessions={[
          { id: "a", preview: "hi", projectId: "foo" },
          { id: "b", preview: "bye", projectId: "foo" },
        ]}
        activeId="a"
        collapsed={{ foo: false }}
      />,
      {},
    );
    await act(async () => {
      await setup.flush();
    });
    const frame = setup.captureCharFrame();
    // active session should show ●
    expect(frame).toMatch(/●/);
    expect(frame).toMatch(/hi/);
    act(() => setup.renderer.destroy());
  });

  test("files view renders the tree and the sidebar-focus hints", async () => {
    const { default: Sidebar } = await import("./sidebar.tsx");
    const nodes = [
      { name: "src", path: "src", kind: "dir" as const, gitStatus: "M" as const, expanded: false,
        loading: false, error: null, truncated: false, children: null },
    ];
    const setup = await testRender(
      <ThemeProvider>
        <Sidebar projects={[]} sessions={[]} view="files" focusMode="sidebar"
          filesProps={{ nodes, selected: 0, error: null, truncated: false, noSession: false, width: 28 }} />
      </ThemeProvider>,
      { width: 80, height: 24 },
    );
    await act(async () => { await setup.flush(); });
    const frame = setup.captureCharFrame();
    expect(frame).toMatch(/Files/);
    expect(frame).toMatch(/r refresh/);
    expect(frame).toMatch(/f threads/);
    expect(frame).toMatch(/tab: compose/);
    expect(frame).toMatch(/src/);
    expect(frame).toMatch(/M/);
    act(() => setup.renderer.destroy());
  });

  test("advertises search hint in sidebar focus and in search mode", async () => {
    const { default: Sidebar } = await import("./sidebar.tsx");
    const baseProps = { projects: [], sessions: [] };
    const filesProps = { nodes: [], selected: 0, error: null, truncated: false, noSession: false, width: 28 };
    const focused = await testRender(
      <ThemeProvider>
        <Sidebar {...baseProps} view="files" focusMode="sidebar" searchMode={null} filesProps={filesProps} />
      </ThemeProvider>,
      { width: 80, height: 24 },
    );
    await act(async () => {
      await focused.flush();
    });
    const frame = focused.captureCharFrame();
    expect(frame).toContain("s search");
    expect(frame).toContain("r refresh");
    expect(frame).toContain("f threads");
    expect(frame).toContain("tab: compose");
    act(() => focused.renderer.destroy());

    const searching = await testRender(
      <ThemeProvider>
        <Sidebar {...baseProps} view="files" focusMode="sidebar" searchMode={{ query: "ma" }} filesProps={filesProps} />
      </ThemeProvider>,
      { width: 80, height: 24 },
    );
    await act(async () => {
      await searching.flush();
    });
    const searchFrame = searching.captureCharFrame();
    expect(searchFrame).toContain("esc back");
    expect(searchFrame).not.toContain("s search");
    act(() => searching.renderer.destroy());
  });

  test("search mode in composer focus does not advertise dead search keys", async () => {
    const { default: Sidebar } = await import("./sidebar.tsx");
    const setup = await testRender(
      <ThemeProvider>
        <Sidebar
          projects={[]}
          sessions={[]}
          view="files"
          focusMode="composer"
          searchMode={{ query: "ma" }}
          filesProps={{ nodes: [], selected: 0, error: null, truncated: false, noSession: false, width: 28 }}
        />
      </ThemeProvider>,
      { width: 80, height: 24 },
    );
    await act(async () => {
      await setup.flush();
    });
    const frame = setup.captureCharFrame();
    expect(frame).not.toContain("esc back");
    expect(frame).not.toContain("s search");
    expect(frame).toContain("tab: sidebar");
    act(() => setup.renderer.destroy());
  });

  test("files view in composer focus does not advertise dead sidebar keys", async () => {
    const { default: Sidebar } = await import("./sidebar.tsx");
    const nodes = [
      { name: "src", path: "src", kind: "dir" as const, gitStatus: "M" as const, expanded: false,
        loading: false, error: null, truncated: false, children: null },
    ];
    const setup = await testRender(
      <ThemeProvider>
        <Sidebar projects={[]} sessions={[]} view="files" focusMode="composer"
          filesProps={{ nodes, selected: 0, error: null, truncated: false, noSession: false, width: 28 }} />
      </ThemeProvider>,
      { width: 80, height: 24 },
    );
    await act(async () => { await setup.flush(); });
    const frame = setup.captureCharFrame();
    expect(frame).toMatch(/Files/);
    expect(frame).not.toMatch(/r refresh/);
    expect(frame).not.toMatch(/f threads/);
    expect(frame).toMatch(/tab: sidebar/);
    expect(frame).toMatch(/src/);
    act(() => setup.renderer.destroy());
  });

  test("files view with default focus does not advertise dead sidebar keys", async () => {
    const { default: Sidebar } = await import("./sidebar.tsx");
    const setup = await testRender(
      <ThemeProvider>
        <Sidebar projects={[]} sessions={[]} view="files" />
      </ThemeProvider>,
      { width: 80, height: 24 },
    );
    await act(async () => { await setup.flush(); });
    const frame = setup.captureCharFrame();
    expect(frame).toMatch(/Files/);
    expect(frame).not.toMatch(/r refresh/);
    expect(frame).not.toMatch(/f threads/);
    expect(frame).toMatch(/tab: sidebar/);
    act(() => setup.renderer.destroy());
  });

  test("persists collapsed state under the shared config dir", async () => {
    const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "aj-sidebar-"));
    const prevXdg = process.env.XDG_CONFIG_HOME;
    process.env.XDG_CONFIG_HOME = tmp;
    try {
      const mod = await import("./sidebar.tsx");
      const cfg = await import("./config.ts");
      // migration: the store resolves the config dir through the shared helper
      expect(mod.getConfigDir).toBe(cfg.getConfigDir);
      mod.saveCollapsed({ p1: true });
      const file = path.join(tmp, "agentj", "projects.json");
      expect(fs.existsSync(file)).toBe(true);
      expect(cfg.readJsonFile<unknown>("projects.json")).toEqual({ collapsed: { p1: true } });
    } finally {
      if (prevXdg === undefined) delete process.env.XDG_CONFIG_HOME;
      else process.env.XDG_CONFIG_HOME = prevXdg;
      fs.rmSync(tmp, { recursive: true, force: true });
    }
  });

  test("parses a pre-refactor projects.json without migration", async () => {
    const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "aj-sidebar-legacy-"));
    const prevXdg = process.env.XDG_CONFIG_HOME;
    process.env.XDG_CONFIG_HOME = tmp;
    try {
      const mod = await import("./sidebar.tsx");
      const dir = path.join(tmp, "agentj");
      fs.mkdirSync(dir, { recursive: true });
      fs.writeFileSync(
        path.join(dir, "projects.json"),
        JSON.stringify({ collapsed: { foo: true }, projects: [{ id: "p1", name: "P1" }] }),
        "utf8",
      );
      expect(mod.loadCollapsed()).toEqual({ foo: true });
      expect(mod.loadProjects()).toEqual([{ id: "p1", name: "P1" }]);
    } finally {
      if (prevXdg === undefined) delete process.env.XDG_CONFIG_HOME;
      else process.env.XDG_CONFIG_HOME = prevXdg;
      fs.rmSync(tmp, { recursive: true, force: true });
    }
  });

  test("saveCollapsed prunes unknown top-level boolean keys", async () => {
    const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "aj-sidebar-prune-"));
    const prevXdg = process.env.XDG_CONFIG_HOME;
    process.env.XDG_CONFIG_HOME = tmp;
    try {
      const mod = await import("./sidebar.tsx");
      const dir = path.join(tmp, "agentj");
      fs.mkdirSync(dir, { recursive: true });
      const file = path.join(dir, "projects.json");
      fs.writeFileSync(file, JSON.stringify({ collapsed: { old: true }, zombie: true, keep: "x" }), "utf8");
      mod.saveCollapsed({ p1: true });
      const raw = JSON.parse(fs.readFileSync(file, "utf8")) as Record<string, unknown>;
      expect(raw.collapsed).toEqual({ p1: true });
      expect("zombie" in raw).toBe(false);
      expect(raw.keep).toBe("x");
      expect(mod.loadCollapsed()).toEqual({ p1: true });
    } finally {
      if (prevXdg === undefined) delete process.env.XDG_CONFIG_HOME;
      else process.env.XDG_CONFIG_HOME = prevXdg;
      fs.rmSync(tmp, { recursive: true, force: true });
    }
  });
});
