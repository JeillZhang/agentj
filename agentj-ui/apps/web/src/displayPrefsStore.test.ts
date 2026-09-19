// agentj-ui/apps/web/src/displayPrefsStore.test.ts
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  DISPLAY_PREFS_DEFAULTS, DISPLAY_PREFS_KEY, FONT_SIZE_LIMITS,
  loadDisplayPrefs, saveDisplayPrefs, subscribeDisplayPrefs,
} from "./displayPrefsStore";

describe("displayPrefsStore", () => {
  beforeEach(() => localStorage.clear());

  it("returns defaults when storage is empty", () => {
    expect(loadDisplayPrefs()).toEqual(DISPLAY_PREFS_DEFAULTS);
  });

  it("falls back field-by-field on malformed values", () => {
    localStorage.setItem(DISPLAY_PREFS_KEY, JSON.stringify({
      fontConversation: 16, fontLeft: "huge", density: "dense", reducedMotion: "yes",
    }));
    const prefs = loadDisplayPrefs();
    expect(prefs.fontConversation).toBe(16);
    expect(prefs.fontLeft).toBe(DISPLAY_PREFS_DEFAULTS.fontLeft);
    expect(prefs.density).toBe("default");
    expect(prefs.reducedMotion).toBe(false);
  });

  it("migrates legacy scale names (sm/md/lg/xl) to pixel values", () => {
    localStorage.setItem(DISPLAY_PREFS_KEY, JSON.stringify({
      fontConversation: "lg", fontLeft: "xl", fontRight: "sm", fontCode: "md",
    }));
    const prefs = loadDisplayPrefs();
    expect(prefs.fontConversation).toBe(17);
    expect(prefs.fontLeft).toBe(16);
    expect(prefs.fontRight).toBe(12);
    expect(prefs.fontCode).toBe(12);
  });

  it("rejects prototype-chain keys masquerading as legacy scales", () => {
    localStorage.setItem(DISPLAY_PREFS_KEY, JSON.stringify({
      fontConversation: "toString", fontLeft: "constructor", fontCode: "__proto__",
    }));
    const prefs = loadDisplayPrefs();
    expect(prefs.fontConversation).toBe(DISPLAY_PREFS_DEFAULTS.fontConversation);
    expect(prefs.fontLeft).toBe(DISPLAY_PREFS_DEFAULTS.fontLeft);
    expect(prefs.fontCode).toBe(DISPLAY_PREFS_DEFAULTS.fontCode);
  });

  it("clamps numeric values to the per-zone limits", () => {
    localStorage.setItem(DISPLAY_PREFS_KEY, JSON.stringify({ fontConversation: 999, fontCode: 2 }));
    const prefs = loadDisplayPrefs();
    expect(prefs.fontConversation).toBe(FONT_SIZE_LIMITS.fontConversation.max);
    expect(prefs.fontCode).toBe(FONT_SIZE_LIMITS.fontCode.min);
  });

  it("persists patches and notifies same-tab subscribers", () => {
    const seen = vi.fn();
    const unsub = subscribeDisplayPrefs(seen);
    const next = saveDisplayPrefs({ fontCode: 14 });
    expect(next.fontCode).toBe(14);
    expect(JSON.parse(localStorage.getItem(DISPLAY_PREFS_KEY)!)).toMatchObject({ fontCode: 14 });
    expect(seen).toHaveBeenCalledWith(expect.objectContaining({ fontCode: 14 }));
    unsub();
  });

  it("clamps patches to the limits when saving", () => {
    expect(saveDisplayPrefs({ fontLeft: 5 }).fontLeft).toBe(FONT_SIZE_LIMITS.fontLeft.min);
  });

  it("re-reads on cross-tab storage events", () => {
    const seen = vi.fn();
    const unsub = subscribeDisplayPrefs(seen);
    window.dispatchEvent(new StorageEvent("storage", { key: DISPLAY_PREFS_KEY }));
    expect(seen).toHaveBeenCalled();
    unsub();
  });
});
