import { readFileSync } from "node:fs";
import { join } from "node:path";
import { beforeEach, expect, test, vi } from "vitest";
import { render, fireEvent, screen, act } from "@testing-library/react";
import WorkspaceShell from "./WorkspaceShell";
import { saveDisplayPrefs } from "../displayPrefsStore";

const layoutCss = readFileSync(join(import.meta.dirname, "layout.css"), "utf8");

/** Concatenates the bodies of every `@media (<query>)` block, balancing braces. */
function mediaBodies(css: string, query: string): string {
  const bodies: string[] = [];
  let from = 0;
  for (;;) {
    const start = css.indexOf(query, from);
    if (start === -1) break;
    const open = css.indexOf("{", start);
    let depth = 0;
    let i = open;
    for (; i < css.length; i += 1) {
      if (css[i] === "{") depth += 1;
      else if (css[i] === "}") {
        depth -= 1;
        if (depth === 0) break;
      }
    }
    bodies.push(css.slice(open + 1, i));
    from = i + 1;
  }
  return bodies.join("\n");
}

beforeEach(() => localStorage.clear());
test("three columns, separators and stored width", () => {
  localStorage.setItem("agentj.layout.leftWidth", "300");
  const { container } = render(
    <WorkspaceShell right={<div>R</div>} rightCollapsed={false} onRightCollapsedChange={() => {}}>
      <div>L</div><div>C</div>
    </WorkspaceShell>);
  const root = container.querySelector(".aj-workspace") as HTMLElement;
  expect(root.style.getPropertyValue("--aj-left-w")).toBe("300px");
  expect(screen.getAllByRole("separator").length).toBe(2);
  expect(container.querySelector(".aj-shell__center")).not.toBeNull();
});
test("keyboard resize persists", () => {
  render(<WorkspaceShell right={<div>R</div>} rightCollapsed={false} onRightCollapsedChange={() => {}}>
    <div>L</div><div>C</div></WorkspaceShell>);
  fireEvent.keyDown(screen.getAllByRole("separator")[0], { key: "ArrowRight" });
  expect(localStorage.getItem("agentj.layout.leftWidth")).toBe("264");
});
test("right toggle persists collapse", () => {
  const onCollapse = vi.fn();
  render(<WorkspaceShell right={<div>R</div>} rightCollapsed={false} onRightCollapsedChange={onCollapse}>
    <div>L</div><div>C</div></WorkspaceShell>);
  fireEvent.keyDown(screen.getByRole("separator", { name: "调整右栏宽度" }), { key: "Enter" });
  expect(onCollapse).toHaveBeenCalledWith(true);
});

test("left sidebar follows the shell column width on wide layouts", () => {
  const wide = mediaBodies(layoutCss, "@media (min-width: 1024px)");
  const match = /\.aj-shell__left\s*>\s*\.aj-sidebar\s*\{([^}]*)\}/.exec(wide);
  expect(match).not.toBeNull();
  expect(match?.[1]).toMatch(/width:\s*100%/);
  expect(match?.[1]).toMatch(/min-width:\s*0/);
  expect(match?.[1]).toMatch(/max-width:\s*none/);
});

test("collapsed right panel drops the shell to the 40px rail on wide layouts", () => {
  const wide = mediaBodies(layoutCss, "@media (min-width: 1024px)");
  const match = /\.aj-workspace--right-collapsed\s+\.aj-shell__right\s*\{([^}]*)\}/.exec(wide);
  expect(match).not.toBeNull();
  expect(match?.[1]).toMatch(/width:\s*40px/);
  expect(match?.[1]).not.toMatch(/flex-basis/);
  expect(match?.[1]).not.toMatch(/min-width/);
  expect(match?.[1]).not.toMatch(/max-width/);
});

test("right column animates width from the base rule with no CSS clamp", () => {
  const base = /\.aj-shell__right\s*\{([^}]*)\}/.exec(layoutCss)?.[1] ?? "";
  expect(base).toMatch(/flex:\s*0 1 auto/);
  expect(base).toMatch(/width:\s*var\(--aj-right-w/);
  expect(base).toMatch(/min-width:\s*0/);
  expect(base).toMatch(/max-width:\s*none/);
});

test("right panel width transition lives in the ≥1024 media and is disabled while dragging", () => {
  const wide = mediaBodies(layoutCss, "@media (min-width: 1024px)");
  const animated = /\.aj-shell__right\s*\{([^}]*transition:\s*width[^}]*)\}/.exec(wide);
  expect(animated).not.toBeNull();
  expect(animated?.[1]).toMatch(/transition:\s*width\s+var\(--aj-motion\)/);
  const guard =
    /\.aj-splitter\[data-side="right"\]\[data-dragging="true"\]\s*~\s*\.aj-shell__right\s*\{([^}]*)\}/.exec(wide);
  expect(guard).not.toBeNull();
  expect(guard?.[1]).toMatch(/transition:\s*none/);
});

test("rightCollapsed marks the root with aj-workspace--right-collapsed", () => {
  const { container } = render(
    <WorkspaceShell right={<div>R</div>} rightCollapsed onRightCollapsedChange={() => {}}>
      <div>L</div><div>C</div>
    </WorkspaceShell>);
  const root = container.querySelector(".aj-workspace") as HTMLElement;
  expect(root.classList.contains("aj-workspace--right-collapsed")).toBe(true);
  expect(root.classList.contains("aj-workspace--right-open")).toBe(false);
});

test("injects display font vars and density", () => {
  render(
    <WorkspaceShell right={<div />} rightCollapsed={false} onRightCollapsedChange={() => {}}>
      <div>l</div>
      <div>c</div>
    </WorkspaceShell>,
  );
  const root = document.querySelector(".aj-workspace") as HTMLElement;
  expect(root.style.getPropertyValue("--aj-font-conv")).toBe("15px");
  expect(root.style.getPropertyValue("--aj-font-left")).toBe("13px");
  expect(root.style.getPropertyValue("--aj-font-right")).toBe("13px");
  expect(root.style.getPropertyValue("--aj-font-code")).toBe("12px");
  expect(root.style.getPropertyValue("--aj-lh-conv")).toBe("1.55");
  expect(root.dataset.density).toBe("default");
  act(() => { saveDisplayPrefs({ fontConversation: "lg", density: "compact", lineHeight: "relaxed" }); });
  expect(root.style.getPropertyValue("--aj-font-conv")).toBe("17px");
  expect(root.style.getPropertyValue("--aj-lh-conv")).toBe("1.8");
  expect(root.dataset.density).toBe("compact");
});
