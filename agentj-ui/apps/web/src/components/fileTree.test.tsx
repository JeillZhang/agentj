import "@testing-library/jest-dom/vitest";
import { expect, test } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import FileTree from "./FileTree";
const tree = [
  { name: "agentj-ui/", path: "agentj-ui", kind: "dir" as const, children: [
    { name: "App.tsx", path: "agentj-ui/App.tsx", kind: "file" as const, git: "M" as const },
  ]},
  { name: "README.md", path: "README.md", kind: "file" as const, git: null },
];
test("expand/collapse and status dots", () => {
  const { container } = render(<FileTree nodes={tree} />);
  expect(screen.queryByText("App.tsx")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: /agentj-ui/ }));
  expect(screen.getByText("App.tsx")).toBeInTheDocument();
  expect(container.querySelector(".status-M")).not.toBeNull();
  fireEvent.click(screen.getByRole("button", { name: /agentj-ui/ }));
  expect(screen.queryByText("App.tsx")).toBeNull();
});
test("empty state and footer", () => {
  render(<FileTree nodes={[]} />);
  expect(screen.getByText("待协议支持")).toBeInTheDocument();
  expect(screen.getByText(/0 个文件 · 0 处变更/)).toBeInTheDocument();
  expect(screen.getByText(/展开全部/)).toBeInTheDocument();
});
