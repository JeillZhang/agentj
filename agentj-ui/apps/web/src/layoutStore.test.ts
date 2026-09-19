import { beforeEach, expect, test } from "vitest";
import { loadLayout, saveLayout, subscribeLayout, loadComposerPrefs, saveComposerPrefs,
         COMPOSER_PERMISSION_MODES,
         loadAskCollapsed, saveAskCollapsed, loadFavorites, toggleFavorite,
         loadRightSections, saveRightSections, setRightSection,
         loadRightSectionSplit, saveRightSectionSplit, subscribeRightSections } from "./layoutStore";
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
test("composer permission defaults to ask and roundtrips", () => {
  expect(loadComposerPrefs().permissionMode).toBe("ask");
  saveComposerPrefs({ permissionMode: "bypass" });
  expect(loadComposerPrefs().permissionMode).toBe("bypass");
  expect(localStorage.getItem("agentj.composer.permissionMode")).toBe("bypass");
  expect(COMPOSER_PERMISSION_MODES).toEqual(["ask", "bypass"]);
});

test("composer permission falls back to ask on an invalid stored value", () => {
  localStorage.setItem("agentj.composer.permissionMode", "bogus");
  expect(loadComposerPrefs().permissionMode).toBe("ask");
});

test("saving a model patch preserves permissionMode", () => {
  saveComposerPrefs({ permissionMode: "bypass" });
  saveComposerPrefs({ model: "deepseek-chat" });
  expect(loadComposerPrefs()).toMatchObject({ model: "deepseek-chat", permissionMode: "bypass" });
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
test("rightCollapsed persists 1/0 wire format", () => {
  saveLayout({ rightCollapsed: true });
  expect(localStorage.getItem("agentj.layout.rightCollapsed")).toBe("1");
  saveLayout({ rightCollapsed: false });
  expect(localStorage.getItem("agentj.layout.rightCollapsed")).toBe("0");
});
test("storage events re-read with clamping", () => {
  const seen: { leftWidth: number }[] = [];
  const unsub = subscribeLayout((s) => seen.push(s as { leftWidth: number }));
  localStorage.setItem("agentj.layout.leftWidth", "999");
  window.dispatchEvent(new StorageEvent("storage", { key: "agentj.layout.leftWidth", newValue: "999" }));
  expect(seen.at(-1)?.leftWidth).toBe(420);
  unsub();
});

test("right sections default to open when absent", () => {
  expect(loadRightSections()).toEqual({ todo: true, changes: true });
});

test("right sections roundtrip and preserve the other section", () => {
  saveRightSections({ todo: false, changes: true });
  expect(loadRightSections()).toEqual({ todo: false, changes: true });
  setRightSection("changes", false);
  expect(loadRightSections()).toEqual({ todo: false, changes: false });
  expect(localStorage.getItem("agentj.layout.rightSections"))
    .toBe(JSON.stringify({ todo: false, changes: false }));
  setRightSection("todo", true);
  expect(loadRightSections()).toEqual({ todo: true, changes: false });
});

test("right sections tolerate malformed values", () => {
  localStorage.setItem("agentj.layout.rightSections", "{not json");
  expect(loadRightSections()).toEqual({ todo: true, changes: true });
  localStorage.setItem("agentj.layout.rightSections", JSON.stringify("bogus"));
  expect(loadRightSections()).toEqual({ todo: true, changes: true });
  localStorage.setItem("agentj.layout.rightSections", JSON.stringify({ todo: "yes" }));
  expect(loadRightSections()).toEqual({ todo: true, changes: true });
  localStorage.setItem("agentj.layout.rightSections", JSON.stringify({ todo: false }));
  expect(loadRightSections()).toEqual({ todo: false, changes: true });
});

test("right-section split defaults null, roundtrips and clamps", () => {
  expect(loadRightSectionSplit()).toBeNull();
  saveRightSectionSplit(0.4);
  expect(loadRightSectionSplit()).toBe(0.4);
  localStorage.setItem("agentj.layout.rightSectionSplit", "2");
  expect(loadRightSectionSplit()).toBe(0.85);
  localStorage.setItem("agentj.layout.rightSectionSplit", "-1");
  expect(loadRightSectionSplit()).toBe(0.15);
  localStorage.setItem("agentj.layout.rightSectionSplit", "bogus");
  expect(loadRightSectionSplit()).toBeNull();
  saveRightSectionSplit(0.6);
  expect(localStorage.getItem("agentj.layout.rightSectionSplit")).toBe("0.6");
  saveRightSectionSplit(null);
  expect(localStorage.getItem("agentj.layout.rightSectionSplit")).toBeNull();
  expect(loadRightSectionSplit()).toBeNull();
});

test("right-section subscribers fire on same-tab updates and matching cross-tab storage", () => {
  const hits: number[] = [];
  const unsub = subscribeRightSections(() => hits.push(hits.length));
  setRightSection("todo", false);
  expect(hits.length).toBe(1);
  window.dispatchEvent(new StorageEvent("storage", { key: "agentj.layout.rightSections" }));
  expect(hits.length).toBe(2);
  window.dispatchEvent(new StorageEvent("storage", { key: null }));
  expect(hits.length).toBe(3);
  window.dispatchEvent(new StorageEvent("storage", { key: "agentj.composer.model" }));
  expect(hits.length).toBe(3);
  unsub();
  setRightSection("todo", true);
  expect(hits.length).toBe(3);
});
