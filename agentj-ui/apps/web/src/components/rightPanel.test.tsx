import { expect, test, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import RightPanel from "./RightPanel";
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
