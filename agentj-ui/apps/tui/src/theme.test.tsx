import { expect, test } from "bun:test";
import { darkPalette, lightPalette, nextTheme } from "./theme.tsx";
import { palettes } from "@agentj/tokens";

test("theme toggles dark to light", () => {
  expect(nextTheme("dark")).toBe("light");
  expect(nextTheme("light")).toBe("dark");
});

test("dark and light palettes alias the @agentj/tokens palettes exactly", () => {
  expect(darkPalette).toEqual(palettes.dark);
  expect(lightPalette).toEqual(palettes.light);
});

test("brand blues, token surfaces, no Tokyo Night leftovers", () => {
  expect(darkPalette.accent).toBe("#679EFE");
  expect(lightPalette.accent).toBe("#4176E6");
  expect(darkPalette.bg).toBe("#151517");
  const dump = JSON.stringify({ dark: darkPalette, light: lightPalette });
  expect(dump).not.toMatch(/#1a1b26|#c0caf5|#7aa2f7|#34548a/i);
  expect("background" in darkPalette).toBe(false);
  expect("foreground" in darkPalette).toBe(false);
});
