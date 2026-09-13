import { expect, test, vi } from "vitest";
import { render, fireEvent } from "@testing-library/react";
import OptionPopover from "./OptionPopover";
test("esc and outside close", () => {
  const onClose = vi.fn();
  const { rerender } = render(<OptionPopover open={false} onClose={onClose}><div>panel</div></OptionPopover>);
  expect(document.querySelector(".pop")).toBeNull();
  rerender(<OptionPopover open onClose={onClose} align="right" width={264} labelledBy="t"><div>panel</div></OptionPopover>);
  const pop = document.querySelector(".pop") as HTMLElement;
  expect(pop.classList.contains("pop-right")).toBe(true);
  expect(pop.style.width).toBe("264px");
  fireEvent.keyDown(document, { key: "Escape" });
  expect(onClose).toHaveBeenCalledTimes(1);
  fireEvent.mouseDown(pop.querySelector("div")!);
  expect(onClose).toHaveBeenCalledTimes(1); // inside: no extra close
  fireEvent.mouseDown(document.body);
  expect(onClose).toHaveBeenCalledTimes(2);
});
test("open focuses the panel when focusable", () => {
  render(
    <OptionPopover open onClose={vi.fn()} tabIndex={-1}>
      <div>panel</div>
    </OptionPopover>,
  );
  const pop = document.querySelector(".pop") as HTMLElement;
  expect(document.activeElement).toBe(pop);
});
test("rerender with a new onClose identity does not steal focus", () => {
  const { rerender } = render(
    <OptionPopover open onClose={vi.fn()} tabIndex={-1}>
      <input aria-label="child" />
    </OptionPopover>,
  );
  const input = document.querySelector("input") as HTMLInputElement;
  input.focus();
  expect(document.activeElement).toBe(input);
  rerender(
    <OptionPopover open onClose={vi.fn()} tabIndex={-1}>
      <input aria-label="child" />
    </OptionPopover>,
  );
  expect(document.activeElement).toBe(input);
});
