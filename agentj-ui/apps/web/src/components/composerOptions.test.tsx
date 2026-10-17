import "@testing-library/jest-dom/vitest";
import { beforeEach, expect, test, vi } from "vitest";
import { act, render, screen, fireEvent } from "@testing-library/react";
import ComposerOptions, { NO_MODEL_HINT } from "./ComposerOptions";
import { createProject, setCurrentProject, updateProject } from "../projectStore";
import { saveComposerPrefs } from "../layoutStore";
beforeEach(() => localStorage.clear());
test("three triggers and mutual exclusion", () => {
  render(<ComposerOptions model="deepseek-chat" />);
  fireEvent.click(screen.getByRole("button", { name: /deepseek-chat/ }));
  expect(document.querySelectorAll(".pop").length).toBe(1);
  fireEvent.click(screen.getByRole("button", { name: /Build/ }));
  expect(document.querySelectorAll(".pop").length).toBe(1);
  expect(screen.getByRole("button", { name: /Build/ })).toHaveAttribute("aria-expanded", "true");
});
test("agent selection persists", () => {
  render(<ComposerOptions model="deepseek-chat" />);
  fireEvent.click(screen.getByRole("button", { name: /Build/ }));
  fireEvent.click(screen.getByText("Plan"));
  expect(localStorage.getItem("agentj.composer.agent")).toBe("plan");
  expect(screen.getByRole("button", { name: /Plan/ })).toBeInTheDocument();
});
test("entropy preset and slider persist", () => {
  render(<ComposerOptions model="deepseek-chat" />);
  fireEvent.click(screen.getByRole("button", { name: /均衡/ }));
  fireEvent.click(screen.getByText(/高温 · 发散/));
  expect(localStorage.getItem("agentj.composer.entropy")).toBe("1.2");
});
test("keyboard navigation selects an option", () => {
  render(<ComposerOptions model="deepseek-chat" />);
  fireEvent.click(screen.getByRole("button", { name: /Build/ }));
  const pop = document.querySelector(".pop") as HTMLElement;
  expect(pop.contains(document.activeElement)).toBe(true);
  fireEvent.keyDown(pop, { key: "ArrowDown" });
  fireEvent.keyDown(pop, { key: "Enter" });
  expect(localStorage.getItem("agentj.composer.agent")).toBe("plan");
});
test("each popover anchors inside its own sel-wrap", () => {
  render(<ComposerOptions model="deepseek-chat" />);
  fireEvent.click(screen.getByRole("button", { name: /deepseek-chat/ }));
  expect(screen.getByRole("button", { name: /deepseek-chat/ }).closest(".sel-wrap")?.querySelector(".pop-model"))
    .not.toBeNull();
  fireEvent.click(screen.getByRole("button", { name: /Build/ }));
  expect(screen.getByRole("button", { name: /Build/ }).closest(".sel-wrap")?.querySelector(".pop-agent"))
    .not.toBeNull();
  fireEvent.click(screen.getByRole("button", { name: /均衡/ }));
  expect(screen.getByRole("button", { name: /均衡/ }).closest(".sel-wrap")?.querySelector(".pop-ent"))
    .not.toBeNull();
});
test("second click on a trigger closes its popover", () => {
  render(<ComposerOptions model="deepseek-chat" />);
  const trigger = screen.getByRole("button", { name: /deepseek-chat/ });
  fireEvent.click(trigger);
  expect(trigger).toHaveAttribute("aria-expanded", "true");
  expect(document.querySelector(".pop-model")).not.toBeNull();
  fireEvent.mouseDown(trigger);
  fireEvent.click(trigger);
  expect(trigger).toHaveAttribute("aria-expanded", "false");
  expect(document.querySelector(".pop")).toBeNull();
  fireEvent.click(trigger);
  expect(trigger).toHaveAttribute("aria-expanded", "true");
  expect(document.querySelector(".pop-model")).not.toBeNull();
});
test("applies project defaults when the current project changes", () => {
  localStorage.clear();
  const alpha = createProject("Alpha");
  updateProject(alpha.id, { defaultModel: "deepseek-chat", defaultAgent: "plan" });
  render(<ComposerOptions />);
  act(() => {
    setCurrentProject(alpha.id);
  });
  expect(localStorage.getItem("agentj.composer.model")).toBe("deepseek-chat");
  expect(localStorage.getItem("agentj.composer.agent")).toBe("plan");
  expect(screen.getByRole("button", { name: /deepseek-chat/ })).toBeTruthy();
});
test("keeps an explicit pick on a same-id project re-dispatch", () => {
  localStorage.clear();
  const alpha = createProject("Alpha");
  updateProject(alpha.id, { defaultModel: "deepseek-chat", defaultAgent: "plan" });
  setCurrentProject(alpha.id);
  render(<ComposerOptions />);
  expect(localStorage.getItem("agentj.composer.agent")).toBe("plan");

  fireEvent.click(screen.getByRole("button", { name: /Plan/ }));
  fireEvent.click(screen.getByText("Build"));
  expect(localStorage.getItem("agentj.composer.agent")).toBe("build");

  act(() => {
    setCurrentProject(alpha.id);
  });
  expect(localStorage.getItem("agentj.composer.agent")).toBe("build");
  expect(screen.getByRole("button", { name: /Build/ })).toBeInTheDocument();
});
test("keeps explicit prefs when the project has no defaults", () => {
  localStorage.clear();
  saveComposerPrefs({ model: "custom-model", agent: "build" });
  const beta = createProject("Beta");
  render(<ComposerOptions />);
  act(() => {
    setCurrentProject(beta.id);
  });
  expect(localStorage.getItem("agentj.composer.model")).toBe("custom-model");
  expect(localStorage.getItem("agentj.composer.agent")).toBe("build");
});
test("applies the current project defaults once on mount", () => {
  localStorage.clear();
  const alpha = createProject("Alpha");
  updateProject(alpha.id, { defaultModel: "deepseek-chat", defaultAgent: "plan" });
  setCurrentProject(alpha.id);
  render(<ComposerOptions />);
  expect(localStorage.getItem("agentj.composer.model")).toBe("deepseek-chat");
  expect(localStorage.getItem("agentj.composer.agent")).toBe("plan");
});

test("permission trigger renders with the default label and opens its popover", () => {
  render(<ComposerOptions model="deepseek-chat" />);
  const trigger = screen.getByRole("button", { name: /逐次确认/ });
  expect(trigger).toHaveAttribute("data-mode", "ask");
  fireEvent.click(trigger);
  expect(screen.getByText("权限模式")).toBeInTheDocument();
  expect(trigger.closest(".sel-wrap")?.querySelector(".pop-perm")).not.toBeNull();
});

test("permission selection persists and updates data-mode", () => {
  render(<ComposerOptions model="deepseek-chat" />);
  fireEvent.click(screen.getByRole("button", { name: /逐次确认/ }));
  fireEvent.click(screen.getByText("自动允许"));
  expect(localStorage.getItem("agentj.composer.permissionMode")).toBe("bypass");
  expect(screen.getByRole("button", { name: /自动允许/ })).toHaveAttribute("data-mode", "bypass");
});

test("permission popover is mutually exclusive with the other selectors", () => {
  render(<ComposerOptions model="deepseek-chat" />);
  const permission = screen.getByRole("button", { name: /逐次确认/ });
  fireEvent.click(permission);
  expect(document.querySelectorAll(".pop").length).toBe(1);
  expect(document.querySelector(".pop-perm")).not.toBeNull();
  fireEvent.click(screen.getByRole("button", { name: /Build/ }));
  expect(document.querySelectorAll(".pop").length).toBe(1);
  expect(document.querySelector(".pop-perm")).toBeNull();
  expect(screen.getByRole("button", { name: /Build/ })).toHaveAttribute("aria-expanded", "true");
  fireEvent.click(permission);
  expect(document.querySelectorAll(".pop").length).toBe(1);
  expect(document.querySelector(".pop-perm")).not.toBeNull();
});

test("permission popover keyboard navigation selects a mode", () => {
  render(<ComposerOptions model="deepseek-chat" />);
  fireEvent.click(screen.getByRole("button", { name: /逐次确认/ }));
  const pop = document.querySelector(".pop") as HTMLElement;
  fireEvent.keyDown(pop, { key: "ArrowDown" });
  fireEvent.keyDown(pop, { key: "Enter" });
  expect(localStorage.getItem("agentj.composer.permissionMode")).toBe("bypass");
});

test("renders the served model list when provided", () => {
  render(
    <ComposerOptions
      model="opencode-go/kimi-k3"
      models={[{ name: "opencode-go/kimi-k3", cap: "—" }]}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: /opencode-go\/kimi-k3/ }));
  expect(screen.getAllByText("opencode-go/kimi-k3").length).toBeGreaterThan(1);
  expect(screen.queryByText("deepseek-chat")).not.toBeInTheDocument();
});

test("hides a persisted model name the kernel no longer serves", () => {
  saveComposerPrefs({ model: "deepseek-chat" });
  render(
    <ComposerOptions model={null} models={[{ name: "opencode-go/kimi-k3", cap: "—" }]} />,
  );
  expect(screen.queryByText("deepseek-chat")).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: /kimi-k3|模型/ })).toBeInTheDocument();
});

test("offers nothing to pick when the kernel served no model list", () => {
  render(<ComposerOptions model="kernel-default" />);
  fireEvent.click(screen.getByRole("button", { name: /kernel-default/ }));
  expect(screen.getByText(NO_MODEL_HINT)).toBeInTheDocument();
  expect(screen.queryAllByRole("option")).toHaveLength(0);
});

test("served model selection persists and updates the trigger label", () => {
  render(
    <ComposerOptions model={null} models={[{ name: "opencode-go/kimi-k3", cap: "—" }]} />,
  );
  fireEvent.click(screen.getByRole("button", { name: /模型/ }));
  fireEvent.click(screen.getByText("opencode-go/kimi-k3"));
  expect(localStorage.getItem("agentj.composer.model")).toBe("opencode-go/kimi-k3");
  expect(screen.getByRole("button", { name: /opencode-go\/kimi-k3/ })).toBeInTheDocument();
});

test("served list marks the kernel default model selected, not the first row", () => {
  render(
    <ComposerOptions
      model="opencode-go/kimi-k3"
      models={[
        { name: "opencode-go/other", cap: "—" },
        { name: "opencode-go/kimi-k3", cap: "—" },
      ]}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: /opencode-go\/kimi-k3/ }));
  const rows = Array.from(document.querySelectorAll(".pop-model .pop-item"));
  const selected = rows.find((r) => r.getAttribute("aria-selected") === "true");
  expect(selected?.textContent).toContain("opencode-go/kimi-k3");
});

test("served list without the current model still selects on Enter", () => {
  render(
    <ComposerOptions
      model="opencode-go/gone"
      models={[{ name: "opencode-go/a", cap: "—" }]}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: /opencode-go\/gone/ }));
  const pop = document.querySelector(".pop") as HTMLElement;
  fireEvent.keyDown(pop, { key: "Enter" });
  expect(localStorage.getItem("agentj.composer.model")).toBe("opencode-go/a");
});

test("selected entry overrides the kernel default label", () => {
  render(
    <ComposerOptions
      model="deepseek-chat"
      selected={{ name: "opencode-go/kimi-k3", provider: "opencode-go" }}
      models={[{ name: "opencode-go/kimi-k3", provider: "opencode-go" }]}
      onSelect={() => {}}
    />,
  );
  expect(screen.getByRole("button", { name: /opencode-go\/kimi-k3/ })).toBeInTheDocument();
});

test("reports the provider-qualified entry through onSelect", () => {
  const onSelect = vi.fn();
  render(
    <ComposerOptions
      model="deepseek-chat"
      onSelect={onSelect}
      models={[{ name: "opencode-go/kimi-k3", provider: "opencode-go" }]}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: /deepseek-chat/ }));
  fireEvent.click(screen.getByText("opencode-go/kimi-k3"));
  expect(onSelect).toHaveBeenCalledWith({ name: "opencode-go/kimi-k3", provider: "opencode-go" });
  expect(localStorage.getItem("agentj.composer.model")).toBe("opencode-go/kimi-k3");
});

test("falls back to the kernel default when nothing is selected", () => {
  render(<ComposerOptions model="deepseek-chat" models={[{ name: "opencode-go/kimi-k3" }]} />);
  expect(screen.getByRole("button", { name: /deepseek-chat/ })).toBeInTheDocument();
});
