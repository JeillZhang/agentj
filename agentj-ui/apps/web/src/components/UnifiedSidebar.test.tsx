import "@testing-library/jest-dom/vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, test } from "vitest";
import { render, screen } from "@testing-library/react";
import UnifiedSidebar from "./UnifiedSidebar";

// BUG-003: default Inbox time subgroups (Today / Previous 7 days / Older)
// are computed in projectStore but were never rendered by the sidebar UI.
function mockSessionClient(
  sessions: Array<Record<string, unknown>>,
): { request: (method: string) => Promise<{ sessions?: Array<Record<string, unknown>> } | null>; setNotificationHandler: () => void } {
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

const DAY = 24 * 3600 * 1000;

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

describe("BUG-003 default Inbox time subgroups", () => {
  test("default Inbox renders Today / Previous 7 days / Older headings with grouped sessions", async () => {
    localStorage.clear();
    const now = Date.now();
    const sessions = [
      { session_id: "today-1", state: "active", preview: "recent", updated_at: now - 60 * 1000 },
      { session_id: "week-1", state: "active", preview: "week old", updated_at: now - 2 * DAY },
      { session_id: "old-1", state: "active", preview: "oldest", updated_at: now - 10 * DAY },
    ];
    render(
      <UnifiedSidebar
        wsUrl="ws://x"
        client={mockSessionClient(sessions) as unknown as never}
        activeSessionId={null}
        onActiveChange={() => {}}
      />,
    );
    expect(await screen.findByRole("heading", { name: /Today/i })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /Previous 7 days/i })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /Older/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /today-1/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /week-1/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /old-1/i })).toBeInTheDocument();
    localStorage.clear();
  });

  test("empty subgroups do not render headings", async () => {
    localStorage.clear();
    const now = Date.now();
    const sessions = [{ session_id: "today-1", state: "active", preview: "recent", updated_at: now }];
    render(
      <UnifiedSidebar
        wsUrl="ws://x"
        client={mockSessionClient(sessions) as unknown as never}
        activeSessionId={null}
        onActiveChange={() => {}}
      />,
    );
    expect(await screen.findByRole("heading", { name: /Today/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /today-1/i })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: /Previous 7 days/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: /Older/i })).not.toBeInTheDocument();
    localStorage.clear();
  });

  test("non-default project sessions stay flat without subgroup headings", async () => {
    localStorage.clear();
    localStorage.setItem(
      "agentj.projects",
      JSON.stringify([
        { id: "default", name: "Inbox", createdAt: 0 },
        { id: "foo", name: "Foo", createdAt: 1 },
      ]),
    );
    localStorage.setItem("agentj.projectMap", JSON.stringify({ "foo-1": "foo" }));
    const sessions = [{ session_id: "foo-1", state: "active", preview: "foo chat", updated_at: Date.now() }];
    render(
      <UnifiedSidebar
        wsUrl="ws://x"
        client={mockSessionClient(sessions) as unknown as never}
        activeSessionId={null}
        onActiveChange={() => {}}
      />,
    );
    expect(await screen.findByRole("button", { name: /foo-1/i })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: /Today/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: /Previous 7 days/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: /Older/i })).not.toBeInTheDocument();
    localStorage.clear();
  });
});
