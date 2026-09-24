import { act, fireEvent, render, screen, within } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import SessionListMenu, { chooseFlyoutSide, clampMenuLeft } from "./SessionListMenu";
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

  it("moves focus into the first menu item when the menu opens", () => {
    render(<SessionListMenu deletableCount={1} onCleanup={() => {}} />);
    open();
    expect(document.activeElement).toBe(screen.getByRole("menuitem", { name: "清理所有会话…" }));
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
    expect(document.activeElement?.textContent).toMatch(/仅收藏/);
  });

  it("roves the flyout with wrap and Home/End", () => {
    render(<SessionListMenu deletableCount={1} onCleanup={() => {}} />);
    open();
    fireEvent.mouseEnter(screen.getByRole("menuitem", { name: /排序/ }));
    const radios = screen.getAllByRole("menuitemradio");
    const flyout = screen.getByRole("menu", { name: "排序" });
    fireEvent.keyDown(flyout, { key: "End" });
    expect(document.activeElement).toBe(radios[radios.length - 1]);
    fireEvent.keyDown(flyout, { key: "ArrowDown" }); // wraps
    expect(document.activeElement).toBe(radios[0]);
  });

  it("moves keyboard focus into the flyout and roves with a single tab stop", () => {
    render(<SessionListMenu deletableCount={1} onCleanup={() => {}} />);
    open();
    // Keyboard-focusing the 过滤 trigger opens the flyout with the first item as
    // its single tab stop, so Tab from the trigger enters the flyout.
    fireEvent.focus(screen.getByRole("menuitem", { name: /过滤/ }));
    const flyout = screen.getByRole("menu", { name: "过滤" });
    const items = within(flyout).getAllByRole("menuitemradio");
    expect(items.filter((el) => el.getAttribute("tabindex") === "0")).toHaveLength(1);
    expect(items[0]).toHaveAttribute("tabindex", "0");
    fireEvent.keyDown(items[0], { key: "ArrowDown" });
    expect(document.activeElement).toBe(items[1]);
    fireEvent.keyDown(items[1], { key: "End" });
    expect(document.activeElement).toBe(items[items.length - 1]);
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

// BUG-008 regression: jsdom has no layout, so the positioning decision is
// extracted into pure functions and locked by unit tests; the stubs below
// replay the bug's measured 390px geometry (trigger left=203, menu width 200).

const ZERO_RECT = (): DOMRect =>
  ({
    x: 0,
    y: 0,
    top: 0,
    left: 0,
    right: 0,
    bottom: 0,
    width: 0,
    height: 0,
    toJSON: () => ({}),
  }) as DOMRect;

const makeRect = (left: number, right: number, top: number, bottom: number): DOMRect =>
  ({
    x: left,
    y: top,
    top,
    left,
    right,
    bottom,
    width: right - left,
    height: bottom - top,
    toJSON: () => ({}),
  }) as DOMRect;

function mockViewport(width: number) {
  const innerWidthSpy = vi.spyOn(window, "innerWidth", "get");
  const rectSpy = vi.spyOn(Element.prototype, "getBoundingClientRect");
  const apply = (w: number) => {
    innerWidthSpy.mockReturnValue(w);
    const triggerLeft = 203;
    const menuLeft = Math.min(triggerLeft, Math.max(8, w - 200 - 8));
    const rowRight = menuLeft + 6 + 188;
    rectSpy.mockImplementation(function (this: Element) {
      const cls = this.classList;
      if (cls?.contains("aj-sidebar__menu-btn")) {
        return makeRect(triggerLeft, triggerLeft + 38, 100, 132);
      }
      if (cls?.contains("aj-menu")) {
        return makeRect(menuLeft, menuLeft + 200, 106, 200);
      }
      const controls = this.getAttribute?.("aria-controls");
      if (controls === "aj-session-menu-filter" || controls === "aj-session-menu-sort") {
        return makeRect(menuLeft + 6, rowRight, 132, 168);
      }
      return ZERO_RECT();
    });
  };
  apply(width);
  return apply;
}

describe("clampMenuLeft (BUG-008)", () => {
  it("keeps the trigger-aligned left when the menu fits the viewport", () => {
    expect(clampMenuLeft(203, 200, 1440)).toBe(203);
  });

  it("clamps the right edge into the 390px viewport", () => {
    expect(clampMenuLeft(203, 200, 390)).toBe(182);
  });

  it("clamps the right edge into the 375px viewport", () => {
    expect(clampMenuLeft(203, 200, 375)).toBe(167);
  });

  it("never places the menu left of the 8px viewport margin", () => {
    expect(clampMenuLeft(2, 200, 390)).toBe(8);
  });
});

describe("chooseFlyoutSide (BUG-008)", () => {
  it("flies right when the row right edge has room", () => {
    expect(chooseFlyoutSide(397, 200, 1440)).toBe("right");
  });

  it("flies left when the right flyout overflows the 390px viewport", () => {
    expect(chooseFlyoutSide(376, 200, 390)).toBe("left");
  });

  it("flies left when the right flyout overflows the 375px viewport", () => {
    expect(chooseFlyoutSide(361, 200, 375)).toBe("left");
  });

  it("keeps flying right when the flyout fits exactly", () => {
    expect(chooseFlyoutSide(184, 200, 390)).toBe("right");
  });
});

describe("SessionListMenu narrow viewport wiring (BUG-008)", () => {
  afterEach(() => vi.restoreAllMocks());

  it("clamps the portaled menu inside the 390px viewport", () => {
    mockViewport(390);
    render(<SessionListMenu deletableCount={1} onCleanup={() => {}} />);
    open();
    const menu = screen.getByRole("menu");
    expect(menu.style.left).toBe("182px");
    expect(menu.style.top).toBe("138px");
  });

  it("flies the filter flyout left when the right side overflows", () => {
    mockViewport(390);
    render(<SessionListMenu deletableCount={1} onCleanup={() => {}} />);
    open();
    fireEvent.mouseEnter(screen.getByRole("menuitem", { name: /过滤/ }));
    const flyout = screen.getByRole("menu", { name: "过滤" });
    expect(flyout.classList.contains("aj-menu__flyout--left")).toBe(true);
  });

  it("recomputes the flyout side when the viewport resizes", () => {
    const apply = mockViewport(390);
    render(<SessionListMenu deletableCount={1} onCleanup={() => {}} />);
    open();
    fireEvent.mouseEnter(screen.getByRole("menuitem", { name: /过滤/ }));
    const flyout = screen.getByRole("menu", { name: "过滤" });
    expect(flyout.classList.contains("aj-menu__flyout--left")).toBe(true);
    apply(1440);
    fireEvent(window, new Event("resize"));
    expect(flyout.classList.contains("aj-menu__flyout--right")).toBe(true);
  });

  it("keeps the right flyout at desktop widths", () => {
    render(<SessionListMenu deletableCount={1} onCleanup={() => {}} />);
    open();
    fireEvent.mouseEnter(screen.getByRole("menuitem", { name: /过滤/ }));
    const flyout = screen.getByRole("menu", { name: "过滤" });
    expect(flyout.classList.contains("aj-menu__flyout--right")).toBe(true);
  });
});
