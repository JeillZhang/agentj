import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";
import SidebarHint from "./SidebarHint";

test("renders contextual hints per view and search state", () => {
  const { rerender } = render(<SidebarHint view="threads" searching={false} />);
  expect(screen.getByText(/j\/k 移动 · Enter 打开 · n 新建 · f 文件视图/)).toBeTruthy();
  rerender(<SidebarHint view="files" searching={true} />);
  expect(screen.getByText(/输入过滤 · Enter 跳转 · Esc 返回/)).toBeTruthy();
  expect(screen.getByTestId("sidebar-hint")).toHaveAttribute("aria-hidden", "true");
});
