import { beforeEach, expect, test } from "vitest";
import { isDemoEnabled, setDemoEnabled, subscribeDemo, DEMO_TODOS, DEMO_CHANGES, DEMO_QUESTION } from "./demoData";
beforeEach(() => { localStorage.clear(); window.history.replaceState(null, "", "/"); });

test("gate precedence", () => {
  expect(isDemoEnabled()).toBe(false);
  localStorage.setItem("agentj.demo", "1");
  expect(isDemoEnabled()).toBe(true);
  localStorage.clear();
  window.history.replaceState(null, "", "/?demo=1");
  expect(isDemoEnabled()).toBe(true);
});

test("setDemoEnabled persists and notifies", () => {
  const seen: boolean[] = [];
  const unsub = subscribeDemo((v) => seen.push(v));
  setDemoEnabled(true);
  expect(localStorage.getItem("agentj.demo")).toBe("1");
  expect(seen).toEqual([true]);
  unsub();
});

test("fixtures match preview states", () => {
  expect(DEMO_TODOS.map((t) => t.status)).toEqual(["completed", "completed", "in_progress", "pending"]);
  expect(DEMO_CHANGES.find((c) => c.path === "SplitPane.tsx")?.additions).toBe(86);
  expect(DEMO_CHANGES.find((c) => c.path === "App.tsx")?.diff).toContain("SplitPane");
  expect(DEMO_QUESTION.options[1].hint).toBe("推荐");
});
