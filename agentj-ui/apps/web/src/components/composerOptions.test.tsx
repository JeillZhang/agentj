import "@testing-library/jest-dom/vitest";
import { beforeEach, expect, test } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import ComposerOptions from "./ComposerOptions";
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
