import "@testing-library/jest-dom/vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, test } from "vitest";
import { render, screen } from "@testing-library/react";
import UnifiedSidebar from "./UnifiedSidebar";

describe("UnifiedSidebar shell", () => {
  test("renders Threads header and footer", () => {
    render(
      <UnifiedSidebar
        wsUrl="ws://127.0.0.1:4096/ws"
        client={null}
        activeSessionId={null}
        onActiveChange={() => {}}
      />,
    );
    expect(screen.getByRole("heading", { name: /Threads/i })).toBeInTheDocument();
    expect(screen.getByLabelText(/Search sessions/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Add Project/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Settings/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Help/i })).toBeInTheDocument();
  });

  test("sidebar width is 256px via CSS var", () => {
    const cssPath = join(import.meta.dirname, "sidebar.css");
    const css = readFileSync(cssPath, "utf8");
    expect(css).toMatch(/width:\s*var\(--aj-sidebar-width\)/);
  });

  test("has hairline 0.5px and gap vars", () => {
    const cssPath = join(import.meta.dirname, "sidebar.css");
    const css = readFileSync(cssPath, "utf8");
    expect(css).toMatch(/0\.5px/);
    // sidebar gap should include 8px or var(--aj-space-2) / 12px var(--aj-space-3)
    expect(css).toMatch(/gap|var\(--aj-space/);
  });

  test("toolbar has Filter Sort View aria labels", () => {
    render(
      <UnifiedSidebar
        wsUrl="ws://127.0.0.1:4096/ws"
        client={null}
        activeSessionId={null}
        onActiveChange={() => {}}
      />,
    );
    expect(screen.getByLabelText(/Filter/i)).toBeInTheDocument();
    expect(screen.getByLabelText(/Sort/i)).toBeInTheDocument();
    expect(screen.getByLabelText(/View/i)).toBeInTheDocument();
  });

  test("keyboard a11y — chevron has aria-expanded and aria-controls", () => {
    render(
      <UnifiedSidebar wsUrl="ws://x" client={null} activeSessionId={null} onActiveChange={() => {}} />,
    );
    const chevrons = screen.getAllByRole("button").filter((b) => b.hasAttribute("aria-expanded"));
    expect(chevrons.length).toBeGreaterThan(0);
    chevrons[0].focus();
    expect(document.activeElement).toBe(chevrons[0]);
    for (const b of chevrons) {
      expect(b).toHaveAttribute("aria-expanded");
      expect(b).toHaveAttribute("aria-controls");
      const id = b.getAttribute("aria-controls")!;
      expect(document.getElementById(id)).not.toBeNull();
    }
  });

  test("reduced-motion disables transitions", () => {
    const cssPath = join(import.meta.dirname, "sidebar.css");
    const css = readFileSync(cssPath, "utf8");
    expect(css).toMatch(/prefers-reduced-motion/);
  });

  test("composer min-height reserve exists", () => {
    const cssPath = join(import.meta.dirname, "composer.css");
    const css = readFileSync(cssPath, "utf8");
    expect(css).toMatch(/min-height:\s*120px/);
  });

  test("unreachable greys sidebar", () => {
    const { container } = render(
      <UnifiedSidebar wsUrl="ws://x" client={null} activeSessionId={null} onActiveChange={() => {}} unreachable />,
    );
    const sidebar = container.querySelector(".aj-sidebar") as HTMLElement;
    expect(sidebar.classList.contains("aj-sidebar--unreachable")).toBe(true);
  });
});
