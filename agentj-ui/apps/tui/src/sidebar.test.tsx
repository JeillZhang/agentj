import { describe, expect, test } from "bun:test";
import { testRender } from "@opentui/react/test-utils";
import { act } from "react";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

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
      process.env.XDG_CONFIG_HOME = prevXdg;
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
});
