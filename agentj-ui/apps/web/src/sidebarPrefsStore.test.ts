import { beforeEach, expect, test } from "vitest";
import {
  SIDEBAR_PREFS_KEY,
  loadSidebarPrefs,
  saveSidebarPrefs,
  subscribeSidebarPrefs,
} from "./sidebarPrefsStore";

beforeEach(() => localStorage.clear());

test("defaults when nothing is stored", () => {
  expect(loadSidebarPrefs()).toEqual({ sort: "updated", filter: "all" });
});

test("roundtrips a single JSON object", () => {
  saveSidebarPrefs({ sort: "created" });
  expect(loadSidebarPrefs()).toEqual({ sort: "created", filter: "all" });
  expect(JSON.parse(localStorage.getItem(SIDEBAR_PREFS_KEY) ?? "null")).toEqual({
    sort: "created",
    filter: "all",
  });
  saveSidebarPrefs({ filter: "favorites" });
  expect(loadSidebarPrefs()).toEqual({ sort: "created", filter: "favorites" });
});

test("notifies same-tab subscribers with the next prefs", () => {
  const seen: unknown[] = [];
  const unsub = subscribeSidebarPrefs((prefs) => seen.push(prefs));
  saveSidebarPrefs({ sort: "created" });
  expect(seen).toEqual([{ sort: "created", filter: "all" }]);
  unsub();
  saveSidebarPrefs({ filter: "favorites" });
  expect(seen.length).toBe(1);
});

test("tolerates malformed or partial stored values", () => {
  localStorage.setItem(SIDEBAR_PREFS_KEY, "{not json");
  expect(loadSidebarPrefs()).toEqual({ sort: "updated", filter: "all" });
  localStorage.setItem(SIDEBAR_PREFS_KEY, JSON.stringify(["updated"]));
  expect(loadSidebarPrefs()).toEqual({ sort: "updated", filter: "all" });
  localStorage.setItem(SIDEBAR_PREFS_KEY, JSON.stringify({ sort: "bogus", filter: 42 }));
  expect(loadSidebarPrefs()).toEqual({ sort: "updated", filter: "all" });
  localStorage.setItem(SIDEBAR_PREFS_KEY, JSON.stringify({ sort: "created" }));
  expect(loadSidebarPrefs()).toEqual({ sort: "created", filter: "all" });
});

test("ignores invalid patch values without clobbering stored prefs", () => {
  saveSidebarPrefs({ sort: "created", filter: "favorites" });
  saveSidebarPrefs({ sort: "bogus" as never });
  expect(loadSidebarPrefs()).toEqual({ sort: "created", filter: "favorites" });
});

test("re-reads on cross-tab storage events for the prefs key", () => {
  const seen: unknown[] = [];
  const unsub = subscribeSidebarPrefs((prefs) => seen.push(prefs));
  localStorage.setItem(SIDEBAR_PREFS_KEY, JSON.stringify({ sort: "created", filter: "favorites" }));
  window.dispatchEvent(
    new StorageEvent("storage", {
      key: SIDEBAR_PREFS_KEY,
      newValue: JSON.stringify({ sort: "created", filter: "favorites" }),
    }),
  );
  expect(seen.at(-1)).toEqual({ sort: "created", filter: "favorites" });
  window.dispatchEvent(new StorageEvent("storage", { key: "agentj.favorites", newValue: "[]" }));
  expect(seen.length).toBe(1);
  unsub();
});
