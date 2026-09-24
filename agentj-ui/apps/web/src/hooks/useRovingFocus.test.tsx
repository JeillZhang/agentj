import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import {
  firstIndex,
  lastIndex,
  moveSelection,
  tabIndexFor,
  useRovingFocus,
} from "./useRovingFocus";

it("moves and clamps a selection index", () => {
  expect(moveSelection(1, 1, 3)).toBe(2);
  expect(moveSelection(2, 1, 3)).toBe(2);
  expect(moveSelection(0, -1, 3)).toBe(0);
  expect(moveSelection(-1, 1, 0)).toBe(-1);
  expect(tabIndexFor(1, 1)).toBe(0);
  expect(tabIndexFor(0, 1)).toBe(-1);
});

describe("moveSelection", () => {
  it("anchors a negative current at index 0 before applying delta", () => {
    expect(moveSelection(-1, -1, 3)).toBe(0);
    expect(moveSelection(-1, 1, 3)).toBe(1);
  });

  it("stays unselected for an empty list", () => {
    expect(moveSelection(0, 1, 0)).toBe(-1);
    expect(moveSelection(2, -1, 0)).toBe(-1);
  });

  it("wraps when asked and clamps by default", () => {
    expect(moveSelection(2, 1, 3, { wrap: true })).toBe(0);
    expect(moveSelection(2, 1, 3)).toBe(2);
    expect(moveSelection(-1, -1, 3, { wrap: true })).toBe(2);
  });
});

describe("firstIndex / lastIndex", () => {
  it("returns -1 for empty lists", () => {
    expect(firstIndex(0)).toBe(-1);
    expect(lastIndex(0)).toBe(-1);
    expect(lastIndex(3)).toBe(2);
  });

  it("returns the first and last valid indices for a populated list", () => {
    expect(firstIndex(3)).toBe(0);
    expect(firstIndex(-1)).toBe(-1);
  });
});

describe("tabIndexFor", () => {
  it("gives the selected row tabIndex 0 and every other row -1", () => {
    expect(tabIndexFor(0, 0)).toBe(0);
    expect(tabIndexFor(2, 0)).toBe(-1);
    expect(tabIndexFor(2, 2)).toBe(0);
  });
});

describe("useRovingFocus", () => {
  it("tracks the selection and clamps moveBy to the list bounds", () => {
    const { result } = renderHook(() => useRovingFocus({ count: 3 }));
    expect(result.current.selected).toBe(-1);

    act(() => result.current.setSelected(0));
    expect(result.current.selected).toBe(0);
    act(() => result.current.moveBy(1));
    expect(result.current.selected).toBe(1);
    act(() => result.current.moveBy(1));
    expect(result.current.selected).toBe(2);
    act(() => result.current.moveBy(1));
    expect(result.current.selected).toBe(2);
  });

  it("stays unselected and ignores moves when the list is empty", () => {
    const { result } = renderHook(() => useRovingFocus({ count: 0 }));
    act(() => result.current.moveBy(1));
    expect(result.current.selected).toBe(-1);
    act(() => result.current.setSelected(2));
    expect(result.current.selected).toBe(-1);
  });

  it("clamps setSelected to the available range", () => {
    const { result } = renderHook(() => useRovingFocus({ count: 3 }));
    act(() => result.current.setSelected(5));
    expect(result.current.selected).toBe(2);
    act(() => result.current.setSelected(-4));
    expect(result.current.selected).toBe(0);
  });

  it("wraps moveBy around the ends when wrap is enabled", () => {
    const { result } = renderHook(() => useRovingFocus({ count: 3, wrap: true }));
    act(() => result.current.setSelected(2));
    act(() => result.current.moveBy(1));
    expect(result.current.selected).toBe(0);
    act(() => result.current.moveBy(-1));
    expect(result.current.selected).toBe(2);
  });
});
