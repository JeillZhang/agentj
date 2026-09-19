import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import "@testing-library/jest-dom/vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import SettingsDisplayPanel from "./SettingsDisplayPanel";
import { DISPLAY_PREFS_DEFAULTS, FONT_SIZE_LIMITS, loadDisplayPrefs } from "../displayPrefsStore";

const css = readFileSync(join(import.meta.dirname, "settingsDialog.css"), "utf8");

describe("SettingsDisplayPanel", () => {
  beforeEach(() => localStorage.clear());

  it("renders one horizontal slider + px number input per font zone", () => {
    render(<SettingsDisplayPanel />);
    expect(screen.getAllByRole("slider")).toHaveLength(4);
    expect(screen.getAllByRole("spinbutton")).toHaveLength(4);
    const slider = screen.getByRole("slider", { name: "对话/轨迹字号" });
    expect(slider).toHaveAttribute("min", String(FONT_SIZE_LIMITS.fontConversation.min));
    expect(slider).toHaveAttribute("max", String(FONT_SIZE_LIMITS.fontConversation.max));
    expect(screen.getByRole("spinbutton", { name: "对话/轨迹字号数值" })).toHaveValue(
      DISPLAY_PREFS_DEFAULTS.fontConversation,
    );
  });

  it("persists a slider font size pick", () => {
    render(<SettingsDisplayPanel />);
    fireEvent.change(screen.getByRole("slider", { name: "对话/轨迹字号" }), {
      target: { value: "20" },
    });
    expect(loadDisplayPrefs().fontConversation).toBe(20);
  });

  it("keeps the four zones independent", () => {
    render(<SettingsDisplayPanel />);
    fireEvent.change(screen.getByRole("slider", { name: "左侧栏字号" }), {
      target: { value: "15" },
    });
    const prefs = loadDisplayPrefs();
    expect(prefs.fontLeft).toBe(15);
    expect(prefs.fontConversation).toBe(DISPLAY_PREFS_DEFAULTS.fontConversation);
    expect(prefs.fontCode).toBe(DISPLAY_PREFS_DEFAULTS.fontCode);
  });

  it("commits a typed px value on blur and clamps out-of-range input on Enter", () => {
    render(<SettingsDisplayPanel />);
    const input = screen.getByRole("spinbutton", { name: "代码字号数值" });
    fireEvent.change(input, { target: { value: "16" } });
    fireEvent.blur(input);
    expect(loadDisplayPrefs().fontCode).toBe(16);
    fireEvent.change(input, { target: { value: "999" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(loadDisplayPrefs().fontCode).toBe(FONT_SIZE_LIMITS.fontCode.max);
    expect(input).toHaveValue(FONT_SIZE_LIMITS.fontCode.max);
  });

  it("restores the current value when the draft is cleared", () => {
    render(<SettingsDisplayPanel />);
    const input = screen.getByRole("spinbutton", { name: "右侧栏字号数值" });
    fireEvent.change(input, { target: { value: "" } });
    fireEvent.blur(input);
    expect(input).toHaveValue(DISPLAY_PREFS_DEFAULTS.fontRight);
    expect(loadDisplayPrefs().fontRight).toBe(DISPLAY_PREFS_DEFAULTS.fontRight);
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
    fireEvent.change(screen.getByRole("slider", { name: "对话/轨迹字号" }), {
      target: { value: "20" },
    });
    fireEvent.click(screen.getByRole("button", { name: "恢复默认" }));
    expect(loadDisplayPrefs()).toEqual(DISPLAY_PREFS_DEFAULTS);
  });

  it("persists density and line height", () => {
    render(<SettingsDisplayPanel />);
    const density = screen.getByRole("group", { name: "界面密度" });
    fireEvent.click(within(density).getByRole("radio", { name: "紧凑" }));
    expect(loadDisplayPrefs().density).toBe("compact");
    const lineHeight = screen.getByRole("group", { name: "行高" });
    fireEvent.click(within(lineHeight).getByRole("radio", { name: "宽松" }));
    expect(loadDisplayPrefs().lineHeight).toBe("relaxed");
  });
});

describe("settingsDialog.css font rows", () => {
  it("lays each zone out as one horizontal grid row (no vertical stack)", () => {
    expect(css).toMatch(/\.aj-settings-font-row\s*\{[^}]*grid-template-columns/);
    expect(css).toMatch(/\.aj-settings-font-row__label/);
    expect(css).toMatch(/\.aj-settings-range\s*\{[^}]*width:\s*100%/);
  });

  it("lays radio option groups out horizontally", () => {
    expect(css).toMatch(/\.aj-settings-options\s*\{[^}]*display:\s*flex/);
  });
});
