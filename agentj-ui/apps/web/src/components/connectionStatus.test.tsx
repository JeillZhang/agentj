import { expect, test } from "vitest";
import "@testing-library/jest-dom/vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import ConnectionStatus from "./ConnectionStatus";

const base = {
  state: "connected" as const,
  label: "已连接 ws://192.168.3.31:4096/ws",
  model: "deepseek-chat",
  modules: ["agent_loop: ready", "tool_executor: ready", "permission: ready"],
};

test("panel is not in the DOM while closed", () => {
  render(<ConnectionStatus {...base} />);
  expect(screen.queryByRole("tooltip")).toBeNull();
  expect(screen.getByLabelText("连接状态")).toHaveAttribute("aria-expanded", "false");
});

test("hovering the light opens the panel with connection, model and modules", () => {
  render(<ConnectionStatus {...base} />);
  fireEvent.mouseEnter(screen.getByTestId("connection-status"));
  const panel = screen.getByRole("tooltip", { name: "连接状态" });
  expect(panel).toHaveTextContent("已连接 ws://192.168.3.31:4096/ws");
  expect(panel).toHaveTextContent("deepseek-chat");
  expect(panel).toHaveTextContent("agent_loop: ready");
  expect(panel).toHaveTextContent("tool_executor: ready");
  expect(panel).toHaveTextContent("permission: ready");
});

test("mouse leave closes the panel", () => {
  render(<ConnectionStatus {...base} />);
  const wrapper = screen.getByTestId("connection-status");
  fireEvent.mouseEnter(wrapper);
  expect(screen.getByRole("tooltip")).toBeInTheDocument();
  fireEvent.mouseLeave(wrapper);
  expect(screen.queryByRole("tooltip")).toBeNull();
});

test("focus opens and blur closes the panel", () => {
  render(<ConnectionStatus {...base} />);
  const trigger = screen.getByLabelText("连接状态");
  fireEvent.focus(trigger);
  expect(screen.getByRole("tooltip")).toBeInTheDocument();
  fireEvent.blur(trigger);
  expect(screen.queryByRole("tooltip")).toBeNull();
});

test("unreachable state drops model/modules and shows the idle dot", () => {
  render(<ConnectionStatus state="unreachable" label="未连接" />);
  fireEvent.mouseEnter(screen.getByTestId("connection-status"));
  const panel = screen.getByRole("tooltip", { name: "连接状态" });
  expect(panel).toHaveTextContent("未连接");
  expect(panel).not.toHaveTextContent("deepseek-chat");
  expect(panel.querySelector(".aj-status-dot")).toHaveAttribute("data-state", "off");
});
