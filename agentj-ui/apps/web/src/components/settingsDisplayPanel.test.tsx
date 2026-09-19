import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import "@testing-library/jest-dom/vitest";
import SettingsDisplayPanel from "./SettingsDisplayPanel";
import { DISPLAY_PREFS_DEFAULTS, loadDisplayPrefs } from "../displayPrefsStore";

describe("SettingsDisplayPanel", () => {
  beforeEach(() => localStorage.clear());

  it("persists a font scale pick", () => {
    render(<SettingsDisplayPanel />);
    const group = screen.getByRole("group", { name: "对话/轨迹" });
    fireEvent.click(within(group).getByRole("radio", { name: "特大" }));
    expect(loadDisplayPrefs().fontConversation).toBe("xl");
  });

  it("toggles reduced motion", () => {
    render(<SettingsDisplayPanel />);
    const sw = screen.getByRole("switch", { name: "减弱动画" });
    expect(sw).toHaveAttribute("aria-checked", "false");
    fireEvent.click(sw);
    expect(loadDisplayPrefs().reducedMotion).toBe(true);
  });

  it("resets to defaults", () => {
    render(<SettingsDisplayPanel />);
    const group = screen.getByRole("group", { name: "对话/轨迹" });
    fireEvent.click(within(group).getByRole("radio", { name: "特大" }));
    fireEvent.click(screen.getByRole("button", { name: "恢复默认" }));
    expect(loadDisplayPrefs()).toEqual(DISPLAY_PREFS_DEFAULTS);
  });

  it("persists density", () => {
    render(<SettingsDisplayPanel />);
    const group = screen.getByRole("group", { name: "界面密度" });
    fireEvent.click(within(group).getByRole("radio", { name: "紧凑" }));
    expect(loadDisplayPrefs().density).toBe("compact");
  });
});
