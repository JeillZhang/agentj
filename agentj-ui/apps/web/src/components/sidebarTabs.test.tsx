import "@testing-library/jest-dom/vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import SidebarTabs from "./SidebarTabs";
test("segmented states and change events", () => {
  const onChange = vi.fn();
  render(<SidebarTabs tab="sessions" onChange={onChange} />);
  expect(screen.getByRole("tab", { name: "会话" })).toHaveAttribute("aria-selected", "true");
  fireEvent.click(screen.getByRole("tab", { name: "文件" }));
  expect(onChange).toHaveBeenCalledWith("files");
});
