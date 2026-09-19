import { act, fireEvent, render, screen } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import { beforeEach, describe, expect, it, vi } from "vitest";
import SessionListMenu from "./SessionListMenu";
import { loadSidebarPrefs } from "../sidebarPrefsStore";

function open(): void {
  fireEvent.click(screen.getByRole("button", { name: "会话列表菜单" }));
}

function renderInSidebar() {
  return render(
    <div className="aj-sidebar">
      <SessionListMenu deletableCount={1} onCleanup={() => {}} />
    </div>,
  );
}

describe("SessionListMenu", () => {
  beforeEach(() => localStorage.clear());

  it("portals the open menu to document.body, outside the sidebar", () => {
    renderInSidebar();
    open();
    const menu = screen.getByRole("menu");
    expect(document.body.contains(menu)).toBe(true);
    expect(menu.closest(".aj-sidebar")).toBeNull();
  });

  it("opens and runs the cleanup action", () => {
    const onCleanup = vi.fn();
    render(<SessionListMenu deletableCount={3} onCleanup={onCleanup} />);
    open();
    fireEvent.click(screen.getByRole("menuitem", { name: "清理所有会话…" }));
    expect(onCleanup).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("disables cleanup when nothing is deletable", () => {
    const onCleanup = vi.fn();
    render(<SessionListMenu deletableCount={0} onCleanup={onCleanup} />);
    open();
    const item = screen.getByRole("menuitem", { name: "清理所有会话…" });
    expect(item).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(item);
    expect(onCleanup).not.toHaveBeenCalled();
  });

  it("hover opens the filter flyout and persists a pick", () => {
    render(<SessionListMenu deletableCount={1} onCleanup={() => {}} />);
    open();
    fireEvent.mouseEnter(screen.getByRole("menuitem", { name: /过滤/ }));
    fireEvent.click(screen.getByRole("menuitemradio", { name: /仅收藏/ }));
    expect(loadSidebarPrefs().filter).toBe("favorites");
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("Escape closes the flyout first, then the menu", () => {
    render(<SessionListMenu deletableCount={1} onCleanup={() => {}} />);
    open();
    fireEvent.mouseEnter(screen.getByRole("menuitem", { name: /排序/ }));
    expect(screen.getByRole("menu", { name: "排序" })).toBeTruthy();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("menu", { name: "排序" })).toBeNull();
    expect(screen.getByRole("menu")).toBeTruthy();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("closes on outside mousedown", () => {
    render(<SessionListMenu deletableCount={1} onCleanup={() => {}} />);
    open();
    fireEvent.mouseDown(document.body);
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("ArrowDown roams the flyout items", () => {
    // import act alongside fireEvent/render/screen
    render(<SessionListMenu deletableCount={1} onCleanup={() => {}} />);
    open();
    fireEvent.mouseEnter(screen.getByRole("menuitem", { name: /过滤/ }));
    fireEvent.keyDown(screen.getByRole("menu", { name: "过滤" }), { key: "ArrowDown" });
    expect(document.activeElement?.textContent).toMatch(/全部/);
  });

  it("closes the flyout after the 120ms leave delay", () => {
    vi.useFakeTimers();
    render(<SessionListMenu deletableCount={1} onCleanup={() => {}} />);
    open();
    fireEvent.mouseEnter(screen.getByRole("menuitem", { name: /过滤/ }));
    fireEvent.mouseLeave(screen.getByRole("menuitem", { name: /过滤/ }));
    act(() => {
      vi.advanceTimersByTime(150);
    });
    expect(screen.queryByRole("menu", { name: "过滤" })).toBeNull();
    vi.useRealTimers();
  });
});
