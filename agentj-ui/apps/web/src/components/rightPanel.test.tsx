import { expect, test, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { render, screen, fireEvent } from "@testing-library/react";
import RightPanel from "./RightPanel";

// helper to read right-panel css via import.meta.dirname (mirrors a11y.test.tsx)
function readCss(name: string): string {
  return readFileSync(join(import.meta.dirname, name), "utf8");
}
test("expanded header and toggle", () => {
  const onToggle = vi.fn();
  const { container } = render(<RightPanel collapsed={false} onToggle={onToggle}><div data-testid="body" /></RightPanel>);
  expect(screen.getByText("运行详情")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: /折叠右栏|展开右栏/ }));
  expect(onToggle).toHaveBeenCalledTimes(1);
  expect(container.querySelector(".aj-right")).not.toBeNull();
});
test("collapsed keeps children mounted", () => {
  const { container } = render(<RightPanel collapsed onToggle={() => {}}><div data-testid="body" /></RightPanel>);
  expect(container.classList.contains("aj-right--collapsed")).toBe(false);
  expect(container.querySelector(".aj-right")?.classList.contains("aj-right--collapsed")).toBe(true);
  expect(container.querySelector("[data-testid=body]")).not.toBeNull();
});
test("css contract — right panel fills height and section titles match", () => {
  const css = readCss("rightpanel.css");
  const ajRight = css.match(/\.aj-right\s*\{[^}]*\}/)?.[0] ?? "";
  expect(ajRight).toMatch(/flex:\s*1/);
  expect(ajRight).toMatch(/min-height:\s*0/);
  const secHead = css.match(/\.sec-head\s*\{[^}]*\}/)?.[0] ?? "";
  expect(secHead).toMatch(/font-size:\s*14px/);
});
