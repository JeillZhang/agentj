import "@testing-library/jest-dom/vitest";
import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import SubscriptionHistory from "./SubscriptionHistory";

describe("SubscriptionHistory", () => {
  it("renders the usage chart and the refresh button", () => {
    const onRefresh = vi.fn();
    render(
      <SubscriptionHistory
        usage={{ windows: [{ kind: "rolling", percent: 42, status: "ok" }] } as never}
        samples={[{ at: 0, windows: [{ kind: "rolling", percent: 42, status: "ok" }] }]}
        historyAt={0}
        onRefresh={onRefresh}
      />,
    );
    expect(screen.getByText("刷新用量")).toBeTruthy();
    fireEvent.click(screen.getByText("刷新用量"));
    expect(onRefresh).toHaveBeenCalledTimes(1);
  });

  it("shows the loading hint without usage", () => {
    render(<SubscriptionHistory usage={null} samples={[]} historyAt={0} onRefresh={() => {}} />);
    expect(screen.getByText("正在获取用量…")).toBeTruthy();
  });
});
