import { expect, test } from "bun:test";
import { nextTheme } from "./theme.tsx";

test("theme toggles dark to light", () => {
  expect(nextTheme("dark")).toBe("light");
  expect(nextTheme("light")).toBe("dark");
});
