// agentj-ui/apps/web/src/displayPrefsStore.test.ts
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  DISPLAY_PREFS_DEFAULTS, DISPLAY_PREFS_KEY,
  loadDisplayPrefs, saveDisplayPrefs, subscribeDisplayPrefs,
} from "./displayPrefsStore";

describe("displayPrefsStore", () => {
  beforeEach(() => localStorage.clear());

  it("returns defaults when storage is empty", () => {
    expect(loadDisplayPrefs()).toEqual(DISPLAY_PREFS_DEFAULTS);
  });

  it("falls back field-by-field on malformed values", () => {
    localStorage.setItem(DISPLAY_PREFS_KEY, JSON.stringify({
      fontConversation: "lg", fontLeft: "huge", density: "dense", reducedMotion: "yes",
    }));
    const prefs = loadDisplayPrefs();
    expect(prefs.fontConversation).toBe("lg");
    expect(prefs.fontLeft).toBe("md");
    expect(prefs.density).toBe("default");
    expect(prefs.reducedMotion).toBe(false);
  });

  it("persists patches and notifies same-tab subscribers", () => {
    const seen = vi.fn();
    const unsub = subscribeDisplayPrefs(seen);
    const next = saveDisplayPrefs({ fontCode: "xl" });
    expect(next.fontCode).toBe("xl");
    expect(JSON.parse(localStorage.getItem(DISPLAY_PREFS_KEY)!)).toMatchObject({ fontCode: "xl" });
    expect(seen).toHaveBeenCalledWith(expect.objectContaining({ fontCode: "xl" }));
    unsub();
  });

  it("re-reads on cross-tab storage events", () => {
    const seen = vi.fn();
    const unsub = subscribeDisplayPrefs(seen);
    window.dispatchEvent(new StorageEvent("storage", { key: DISPLAY_PREFS_KEY }));
    expect(seen).toHaveBeenCalled();
    unsub();
  });
});
