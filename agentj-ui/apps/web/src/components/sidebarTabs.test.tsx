import "@testing-library/jest-dom/vitest";
import { useState } from "react";
import { render, screen, fireEvent } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import SidebarTabs, { type SidebarTab } from "./SidebarTabs";
import { moveSelection } from "../hooks/useRovingFocus";

vi.mock("../hooks/useRovingFocus", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../hooks/useRovingFocus")>();
  return { ...actual, moveSelection: vi.fn(actual.moveSelection) };
});
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

function ControlledSidebarTabs() {
  const [tab, setTab] = useState<SidebarTab>("sessions");
  return <SidebarTabs tab={tab} onChange={setTab} />;
}

test("wraps with shared helpers and keeps a single tab stop", () => {
  render(<ControlledSidebarTabs />);
  const tablist = screen.getByRole("tablist");
  fireEvent.keyDown(tablist, { key: "ArrowLeft" }); // wraps sessions -> files
  expect(document.activeElement).toHaveAttribute("data-tab", "files");
  expect(screen.getByRole("tab", { name: "文件" })).toHaveAttribute("tabindex", "0");
  expect(screen.getByRole("tab", { name: "会话" })).toHaveAttribute("tabindex", "-1");
  fireEvent.keyDown(tablist, { key: "ArrowRight" }); // wraps files -> sessions
  expect(document.activeElement).toHaveAttribute("data-tab", "sessions");
  expect(screen.getByRole("tab", { name: "会话" })).toHaveAttribute("tabindex", "0");
});

test("routes arrow navigation through the shared roving helper", () => {
  vi.mocked(moveSelection).mockClear();
  render(<SidebarTabs tab="sessions" onChange={vi.fn()} />);
  fireEvent.keyDown(screen.getByRole("tablist"), { key: "ArrowLeft" });
  expect(vi.mocked(moveSelection)).toHaveBeenCalledWith(0, -1, 2, { wrap: true });
});
