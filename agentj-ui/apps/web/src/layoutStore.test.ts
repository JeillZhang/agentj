import { beforeEach, expect, test } from "vitest";
import { loadLayout, saveLayout, subscribeLayout, loadComposerPrefs, saveComposerPrefs,
         loadAskCollapsed, saveAskCollapsed, loadFavorites, toggleFavorite } from "./layoutStore";
beforeEach(() => localStorage.clear());
test("defaults and clamps", () => {
  expect(loadLayout()).toEqual({ leftWidth: 256, rightWidth: 320, leftTab: "sessions", rightCollapsed: false });
  localStorage.setItem("agentj.layout.leftWidth", "999");
  expect(loadLayout().leftWidth).toBe(420);
  localStorage.setItem("agentj.layout.leftWidth", "abc");
  expect(loadLayout().leftWidth).toBe(256);
  localStorage.setItem("agentj.layout.rightWidth", "100");
  expect(loadLayout().rightWidth).toBe(260);
});
test("roundtrip, invalid leftTab fallback and events", () => {
  const seen: unknown[] = [];
  const unsub = subscribeLayout((s) => seen.push(s));
  saveLayout({ leftWidth: 300 });
  expect(loadLayout().leftWidth).toBe(300);
  expect(seen.length).toBe(1);
  localStorage.setItem("agentj.layout.leftTab", "bogus");
  expect(loadLayout().leftTab).toBe("sessions");
  unsub();
});
test("composer prefs clamp and enum fallback", () => {
  localStorage.setItem("agentj.composer.entropy", "9");
  localStorage.setItem("agentj.composer.agent", "bogus");
  const p = loadComposerPrefs();
  expect(p.entropy).toBe(2);
  expect(p.agent).toBe("build");
  saveComposerPrefs({ entropy: 1.2, agent: "plan" });
  expect(loadComposerPrefs().entropy).toBe(1.2);
  expect(loadComposerPrefs().agent).toBe("plan");
});
test("ask collapsed and favorites toggle", () => {
  expect(loadAskCollapsed()).toBe(false);
  saveAskCollapsed(true);
  expect(loadAskCollapsed()).toBe(true);
  localStorage.setItem("agentj.favorites", JSON.stringify(["a"]));
  toggleFavorite("a");
  expect(loadFavorites()).toEqual([]);
  toggleFavorite("b");
  expect(loadFavorites()).toEqual(["b"]);
});
test("storage events re-read with clamping", () => {
  const seen: { leftWidth: number }[] = [];
  const unsub = subscribeLayout((s) => seen.push(s as { leftWidth: number }));
  localStorage.setItem("agentj.layout.leftWidth", "999");
  window.dispatchEvent(new StorageEvent("storage", { key: "agentj.layout.leftWidth", newValue: "999" }));
  expect(seen.at(-1)?.leftWidth).toBe(420);
  unsub();
});
