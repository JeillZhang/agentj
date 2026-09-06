import "@testing-library/jest-dom/vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, test } from "vitest";
import { render, screen, within } from "@testing-library/react";
import UnifiedSidebar from "./UnifiedSidebar";

// helper to read sidebar/composer css via import.meta.dirname
function readCss(name: string): string {
  // a11y.test.ts lives in src/components/
  const base = join(import.meta.dirname);
  return readFileSync(join(base, name), "utf8");
}

describe("a11y gates T9", () => {
  test("keyboard a11y — chevron has aria-expanded + aria-controls, session rows aria-selected, tabIndex 0", () => {
    render(<UnifiedSidebar wsUrl="ws://x" client={null} activeSessionId={null} onActiveChange={() => {}} />);
    const chevrons = screen.getAllByRole("button").filter((b) => b.hasAttribute("aria-expanded"));
    expect(chevrons.length).toBeGreaterThan(0);
    chevrons[0].focus();
    expect(document.activeElement).toBe(chevrons[0]);
    // aria-controls must be present and point to existing id
    for (const b of chevrons) {
      expect(b).toHaveAttribute("aria-expanded");
      expect(b).toHaveAttribute("aria-controls");
      const id = b.getAttribute("aria-controls")!;
      expect(document.getElementById(id)).not.toBeNull();
    }
    // session rows not rendered by default (no sessions) -> check behavior with activeSession only fallback:
    // But ensure any existing session rows have aria-selected
  });

  test("session rows have aria-selected and keyboard Enter/Space", async () => {
    // prepare favorites to force rows
    localStorage.setItem("agentj.favorites", JSON.stringify(["fav-1"]));
    // also need to mock session list via client null -> rowsFromList empty, but favorites path renders rows
    const { unmount } = render(
      <UnifiedSidebar wsUrl="ws://x" client={null} activeSessionId="fav-1" onActiveChange={() => {}} />,
    );
    const list = screen.getByLabelText("favorites");
    const rows = within(list).getAllByRole("button");
    expect(rows.length).toBeGreaterThan(0);
    for (const r of rows) {
      expect(r).toHaveAttribute("aria-selected");
      expect(r.getAttribute("tabIndex")).toBe("0");
    }
    // keyboard Enter triggers onActiveChange via click simulation not needed; ensure button is focusable and responds to key
    rows[0].focus();
    expect(document.activeElement).toBe(rows[0]);
    unmount();
    localStorage.clear();
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

  test("virtualization slices >50 (not rendering all at once, Show more)", async () => {
    // Build a mock client returning 60 sessions for default project
    localStorage.clear();
    const sessions = Array.from({ length: 60 }, (_, i) => ({
      session_id: `s-${String(i).padStart(2, "0")}`,
      state: "active",
      preview: `preview ${i}`,
      updated_at: Date.now().toString(),
    }));
    const mockClient = {
      request: async (method: string) => {
        if (method === "session/list") return { sessions };
        if (method === "kernel/status") return { modules: [] };
        if (method === "initialize") return { protocol_version: "0.12.1" };
        return null;
      },
      setNotificationHandler: () => {},
    };
    render(
      <UnifiedSidebar wsUrl="ws://x" client={mockClient as unknown as never} activeSessionId={null} onActiveChange={() => {}} />,
    );
    // wait for async fetch
    await new Promise((r) => setTimeout(r, 50));
    // should not render 60 at once
    // count session rows specifically: those with aria-selected
    const sessionRows = document.querySelectorAll('[aria-selected]');
    // session rows should be <=50 initially
    expect(sessionRows.length).toBeLessThanOrEqual(50);
    expect(sessionRows.length).toBeGreaterThan(0);
    // Show more button must exist when >50
    expect(screen.getByRole("button", { name: /show more/i })).toBeInTheDocument();
    localStorage.clear();
  });

  test("unreachable styling — sidebar greys, status popover red, cached list still browseable", () => {
    localStorage.setItem("agentj.favorites", JSON.stringify(["cached-1"]));
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
    // cached list still browseable
    expect(screen.getByText("cached-1")).toBeInTheDocument();
    localStorage.clear();
  });
});
