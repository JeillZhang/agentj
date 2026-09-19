import "@testing-library/jest-dom/vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { beforeEach, describe, expect, test } from "vitest";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import UnifiedSidebar from "./UnifiedSidebar";
import TopbarMenu from "./TopbarMenu";
import WorkspaceShell from "./WorkspaceShell";

// helper to read sidebar/composer css via import.meta.dirname
function readCss(name: string): string {
  // a11y.test.ts lives in src/components/
  const base = join(import.meta.dirname);
  return readFileSync(join(base, name), "utf8");
}

/** 60-session mock client used by the virtualization regression case. */
function sixtySessionClient() {
  const sessions = Array.from({ length: 60 }, (_, i) => ({
    session_id: `s-${String(i).padStart(2, "0")}`,
    state: "active",
    preview: `preview ${i}`,
    updated_at: Date.now().toString(),
  }));
  return {
    request: async (method: string) => {
      if (method === "session/list") return { sessions };
      if (method === "kernel/status") return { modules: [] };
      if (method === "initialize") return { protocol_version: "0.12.1" };
      return null;
    },
    setNotificationHandler: () => {},
  };
}

describe("a11y gates T9", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  test("keyboard a11y — project flyout trigger exposes aria-haspopup + aria-expanded and focuses its items", () => {
    render(<TopbarMenu onConnect={() => {}} onContinue={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "更多" }));
    const trigger = screen.getByRole("menuitem", { name: "项目" });
    expect(trigger).toHaveAttribute("aria-haspopup", "menu");
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    act(() => {
      trigger.focus();
    });
    expect(document.activeElement).toBe(trigger);
    expect(trigger).toHaveAttribute("aria-expanded", "true");
    const flyout = screen.getByRole("menu", { name: "项目" });
    expect(flyout).toBeInTheDocument();
    const first = within(flyout).getAllByRole("menuitemradio")[0];
    first.focus();
    fireEvent.keyDown(first, { key: "ArrowDown" });
    expect(flyout.contains(document.activeElement)).toBe(true);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(trigger).toHaveAttribute("aria-expanded", "false");
  });

  test("sidebar rows stay aria-selected and focusable (60 sessions → ≤50 rows, 显示更多 present)", async () => {
    render(
      <UnifiedSidebar
        wsUrl="ws://x"
        client={sixtySessionClient() as unknown as never}
        activeSessionId={null}
        onActiveChange={() => {}}
      />,
    );
    await waitFor(() => {
      expect(document.querySelectorAll(".aj-session-list [aria-selected]").length).toBeGreaterThan(0);
    });
    // flat list pages at 50 rows — never all 60 in one list.
    // Scoped to session lists: the sidebar tabs also carry [aria-selected].
    const sessionRows = document.querySelectorAll(".aj-session-list [aria-selected]");
    expect(sessionRows.length).toBeLessThanOrEqual(50);
    expect(sessionRows.length).toBeGreaterThan(0);
    for (const r of sessionRows) {
      expect(r.getAttribute("tabIndex")).toBe("0");
    }
    // overflow affordance must exist once either list exceeds the 50-row page
    expect(screen.getAllByRole("button", { name: /显示更多/ }).length).toBeGreaterThan(0);
  });

  test("reduced-motion disables transitions in sidebar.css and composer.css", () => {
    const sidebar = readCss("sidebar.css");
    expect(sidebar).toMatch(/prefers-reduced-motion/);
    const composer = readCss("composer.css");
    expect(composer).toMatch(/prefers-reduced-motion/);
    // sidebar drawer transitions disabled: should contain transition:none or transform
    expect(sidebar).toMatch(/transition:\s*none/);
  });

  test("composer min-height reserve (CLS <0.1)", () => {
    const composer = readCss("composer.css");
    expect(composer).toMatch(/min-height:\s*120px/);
  });

  test("unreachable styling — sidebar greys, status popover red", () => {
    const { container } = render(
      <UnifiedSidebar wsUrl="ws://x" client={null} activeSessionId={null} onActiveChange={() => {}} unreachable />,
    );
    const sidebar = container.querySelector(".aj-sidebar") as HTMLElement;
    expect(sidebar).not.toBeNull();
    expect(sidebar.classList.contains("aj-sidebar--unreachable")).toBe(true);
    const css = readCss("sidebar.css");
    expect(css).toMatch(/aj-sidebar--unreachable/);
    expect(css).toMatch(/opacity:\s*0\.6/);
    expect(css).toMatch(/var\(--aj-error\)/);
  });

  test("separator exposes full aria contract and keyboard control", () => {
    render(
      <WorkspaceShell right={<div>R</div>} rightCollapsed={false} onRightCollapsedChange={() => {}}>
        <div>L</div>
        <div>C</div>
      </WorkspaceShell>,
    );
    const sep = screen.getByRole("separator", { name: "调整左栏宽度" });
    expect(sep).toHaveAttribute("aria-valuemin", "200");
    expect(sep).toHaveAttribute("aria-valuemax", "420");
    fireEvent.keyDown(sep, { key: "ArrowRight" });
    expect(sep).toHaveAttribute("aria-valuenow", "264");
  });

  test("responsive layout rules present", () => {
    const layout = readCss("layout.css");
    expect(layout).toMatch(/@media\s*\(max-width:\s*1023px\)/);
    expect(layout).toMatch(/display:\s*none/); // splitters hidden on narrow
  });
});
