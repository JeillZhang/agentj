import "@testing-library/jest-dom/vitest";
import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import SubscriptionUsage from "./SubscriptionUsage";

describe("SubscriptionUsage", () => {
  it("renders the mapped sentence for a known error kind", () => {
    render(
      <SubscriptionUsage usage={{ windows: [], error: { kind: "unavailable" } } as never} />,
    );
    expect(screen.getByText("用量暂时无法获取（上游响应无法识别）")).toBeTruthy();
  });

  it("keeps the raw RPC text visible next to the mapped sentence", () => {
    render(
      <SubscriptionUsage
        usage={
          {
            windows: [],
            error: { kind: "unavailable", message: "-32602: InvalidParams" },
          } as never
        }
      />,
    );
    const raw = screen.getByText("-32602: InvalidParams");
    expect(raw).toHaveClass("aj-sub__error--raw");
  });

  it("shows the raw text alone for an unrecognised kind", () => {
    render(
      <SubscriptionUsage
        usage={{ windows: [], error: { kind: "something-new", message: "boom" } } as never}
      />,
    );
    expect(screen.getByText("boom")).toHaveClass("aj-sub__error--raw");
    expect(screen.queryByText("用量暂时无法获取（上游响应无法识别）")).toBeNull();
  });

  it("never renders an inherited Object.prototype member as a sentence", () => {
    render(
      <SubscriptionUsage
        usage={{ windows: [], error: { kind: "toString", message: "boom" } } as never}
      />,
    );
    expect(screen.getByText("boom")).toBeTruthy();
    expect(document.querySelector(".aj-sub__usage")?.textContent).not.toContain("function");
  });
});
