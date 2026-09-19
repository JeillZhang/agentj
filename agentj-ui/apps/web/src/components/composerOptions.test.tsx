import "@testing-library/jest-dom/vitest";
import { beforeEach, expect, test } from "vitest";
import { act, render, screen, fireEvent } from "@testing-library/react";
import ComposerOptions from "./ComposerOptions";
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
