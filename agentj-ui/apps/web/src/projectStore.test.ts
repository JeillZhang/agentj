import { beforeEach, describe, expect, test } from "vitest";
import {
  loadProjects,
  createProject,
  assignSession,
  getProjectForSession,
  renameProject,
  groupSessionsByProject,
  loadCollapsed,
  setCollapsed,
  loadCurrentProject,
  setCurrentProject,
} from "./projectStore";

beforeEach(() => localStorage.clear());

describe("projectStore", () => {
  test("defaults to Inbox", () => {
    expect(loadProjects()).toEqual([{ id: "default", name: "Inbox", createdAt: expect.any(Number) }]);
  });

  test("corrupted JSON fallback", () => {
    localStorage.setItem("agentj.projects", "not-json");
    expect(loadProjects()).toEqual(expect.arrayContaining([expect.objectContaining({ id: "default" })]));
  });

  test("create and assign with dedup", () => {
    const p = createProject("Foo");
    expect(loadProjects().some((x) => x.name === "Foo")).toBe(true);
    const p2 = createProject("Foo");
    expect(p2.name).toBe("Foo (2)");
    expect(p2.id).not.toBe(p.id);
    assignSession("abc", p.id);
    expect(getProjectForSession("abc")).toBe(p.id);
    expect(getProjectForSession("unknown")).toBe("default");
  });

  test("createProject dispatches agentj:projects-updated for same-tab sync", () => {
    const seen: string[] = [];
    const handler = (e: Event) => seen.push(e.type);
    window.addEventListener("agentj:projects-updated", handler);
    try {
      createProject("Foo");
    } finally {
      window.removeEventListener("agentj:projects-updated", handler);
    }
    expect(seen).toContain("agentj:projects-updated");
  });

  test("setCurrentProject dispatches agentj:current-project-updated with detail.id", () => {
    const details: unknown[] = [];
    const handler = (e: Event) => details.push((e as CustomEvent).detail);
    window.addEventListener("agentj:current-project-updated", handler);
    try {
      setCurrentProject("abc");
    } finally {
      window.removeEventListener("agentj:current-project-updated", handler);
    }
    expect(details).toContainEqual({ id: "abc" });
  });

  test("rename and empty rejected", () => {
    const p = createProject("Foo");
    renameProject(p.id, "Bar");
    expect(loadProjects().find((x) => x.id === p.id)?.name).toBe("Bar");
    expect(() => createProject("")).toThrow();
    expect(() => createProject("   ")).toThrow();
    expect(() => renameProject(p.id, "")).toThrow();
    expect(() => renameProject(p.id, "   ")).toThrow();
  });

  test("rename dedup suffix", () => {
    const a = createProject("Foo");
    const b = createProject("Bar");
    renameProject(b.id, "Foo");
    expect(loadProjects().find((x) => x.id === b.id)?.name).toBe("Foo (2)");
    // ensure original still Foo
    expect(loadProjects().find((x) => x.id === a.id)?.name).toBe("Foo");
  });

  test("dedup creates (3) suffix", () => {
    createProject("Foo");
    createProject("Foo");
    const p3 = createProject("Foo");
    expect(p3.name).toBe("Foo (3)");
  });

  test("time grouping fallback", () => {
    const now = Date.now();
    const sessions = [
      { id: "a", updatedAt: now },
      { id: "b", updatedAt: now - 2 * 24 * 3600 * 1000 },
      { id: "c", updatedAt: now - 10 * 24 * 3600 * 1000 },
    ];
    const groups = groupSessionsByProject(sessions as any);
    expect(groups["default"].subgroups["Today"].length).toBe(1);
    expect(groups["default"].subgroups["Previous 7 days"].length).toBe(1);
    expect(groups["default"].subgroups["Older"].length).toBe(1);
  });

  test("groupSessionsByProject respects projectMap", () => {
    const p = createProject("Foo");
    assignSession("s1", p.id);
    const now = Date.now();
    const sessions = [
      { id: "s1", updatedAt: now },
      { id: "s2", updatedAt: now },
    ];
    const groups = groupSessionsByProject(sessions as any);
    expect(groups[p.id].sessions.length).toBe(1);
    expect(groups["default"].sessions.length).toBe(1);
  });

  test("collapsed persistence", () => {
    setCollapsed({ foo: true });
    expect(loadCollapsed()).toEqual({ foo: true });
    // reload simulation - localStorage persists
    expect(JSON.parse(localStorage.getItem("agentj.projectCollapsed")!)).toEqual({ foo: true });
  });

  test("currentProject persistence + stale fallback", () => {
    const p = createProject("Foo");
    setCurrentProject(p.id);
    expect(loadCurrentProject()).toBe(p.id);
    expect(localStorage.getItem("agentj.currentProject")).toBe(p.id);
    // stale id fallback to default
    localStorage.setItem("agentj.currentProject", "stale-id-xyz");
    expect(loadCurrentProject()).toBe("default");
    // default always valid
    localStorage.removeItem("agentj.currentProject");
    expect(loadCurrentProject()).toBe("default");
  });

  test("getProjectForSession stale map fallback", () => {
    // manually set a stale project id in map
    localStorage.setItem("agentj.projectMap", JSON.stringify({ abc: "stale-proj-999" }));
    // loadProjects has only default, so stale should fallback to default
    expect(getProjectForSession("abc")).toBe("default");
  });

  test("assignSession persists to localStorage", () => {
    const p = createProject("Foo");
    assignSession("abc", p.id);
    const raw = JSON.parse(localStorage.getItem("agentj.projectMap")!);
    expect(raw["abc"]).toBe(p.id);
  });

  test("loadProjects persists default when empty", () => {
    localStorage.clear();
    loadProjects();
    const raw = localStorage.getItem("agentj.projects");
    expect(raw).not.toBeNull();
    expect(JSON.parse(raw!)).toEqual(expect.arrayContaining([expect.objectContaining({ id: "default" })]));
  });
});
