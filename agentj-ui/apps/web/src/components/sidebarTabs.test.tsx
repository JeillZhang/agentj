import "@testing-library/jest-dom/vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import SidebarTabs from "./SidebarTabs";
test("segmented states and change events", () => {
  const onChange = vi.fn();
  render(<SidebarTabs tab="sessions" onChange={onChange} />);
  expect(screen.getByRole("tab", { name: "会话" })).toHaveAttribute("aria-selected", "true");
  fireEvent.click(screen.getByRole("tab", { name: "文件" }));
  expect(onChange).toHaveBeenCalledWith("files");
});

test("switches tabs with arrow keys", () => {
  const onChange = vi.fn();
  render(<SidebarTabs tab="sessions" onChange={onChange} />);
  fireEvent.keyDown(screen.getByRole("tablist"), { key: "ArrowRight" });
  expect(onChange).toHaveBeenCalledWith("files");
});

test("switches back to sessions with ArrowLeft", () => {
  const onChange = vi.fn();
  render(<SidebarTabs tab="files" onChange={onChange} />);
  fireEvent.keyDown(screen.getByRole("tablist"), { key: "ArrowLeft" });
  expect(onChange).toHaveBeenCalledWith("sessions");
});

test("moves focus to the newly selected tab", () => {
  const onChange = vi.fn();
  render(<SidebarTabs tab="sessions" onChange={onChange} />);
  fireEvent.keyDown(screen.getByRole("tablist"), { key: "ArrowRight" });
  expect(screen.getByRole("tab", { name: "文件" })).toHaveFocus();
});

test("uses roving tabIndex on the selected tab", () => {
  render(<SidebarTabs tab="files" onChange={vi.fn()} />);
  expect(screen.getByRole("tab", { name: "文件" })).toHaveAttribute("tabindex", "0");
  expect(screen.getByRole("tab", { name: "会话" })).toHaveAttribute("tabindex", "-1");
});
