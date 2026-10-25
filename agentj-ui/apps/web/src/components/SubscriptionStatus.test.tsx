import "@testing-library/jest-dom/vitest";
import { afterEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import type { UsageWindow } from "@agentj/protocol";
import type { SubscriptionStatusData } from "../hooks/useSubscriptionStatus";
import SubscriptionStatus from "./SubscriptionStatus";

function win(kind: string, percent: number): UsageWindow {
  return { kind, percent, status: "ok", resets_at: null } as unknown as UsageWindow;
}

function data(over: Partial<SubscriptionStatusData> = {}): SubscriptionStatusData {
  return {
    state: "ok",
    windows: [win("rolling", 12)],
    worstPercent: 12,
    fetchedAt: 0,
    credential: { kind: "api_key", key_hint: "…1234" },
    warnPercent: 70,
    stale: false,
    errorKind: null,
    error: null,
    loading: false,
    refresh: async () => {},
    ...over,
  };
}

function open(): void {
  fireEvent.mouseEnter(screen.getByTestId("subscription-status"));
}

describe("SubscriptionStatus", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("exposes one trigger whose accessible name names the subscription", () => {
    render(<SubscriptionStatus status={data()} />);
    const buttons = screen.getAllByRole("button", { name: /订阅/ });
    expect(buttons).toHaveLength(1);
    expect(buttons[0]).toHaveAttribute("aria-expanded", "false");
  });

  it("paints the light from the graded state", () => {
    const { rerender } = render(<SubscriptionStatus status={data({ state: "ok" })} />);
    const light = () => screen.getByTestId("sub-dot");
    expect(light()).toHaveAttribute("data-state", "ok");
    rerender(<SubscriptionStatus status={data({ state: "warn", worstPercent: 74 })} />);
    expect(light()).toHaveAttribute("data-state", "warn");
    rerender(<SubscriptionStatus status={data({ state: "error", worstPercent: 92 })} />);
    expect(light()).toHaveAttribute("data-state", "error");
  });

  it("opens on hover with one row per window and the fetch age", () => {
    vi.useFakeTimers();
    const now = Date.now();
    render(<SubscriptionStatus status={data({ fetchedAt: now })} />);
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
    open();
    expect(screen.getByRole("tooltip")).toBeInTheDocument();
    expect(screen.getByTestId("sub-row-rolling")).toHaveTextContent("12%");
    expect(screen.getByTestId("sub-age")).toHaveTextContent("<1 分钟");
  });

  it("marks a seven minute old payload as stale with a tilde prefix", () => {
    vi.useFakeTimers();
    const now = Date.now();
    render(<SubscriptionStatus status={data({ fetchedAt: now - 7 * 60_000 })} />);
    open();
    expect(screen.getByTestId("sub-age")).toHaveTextContent("~7 分钟前");
    expect(screen.getByTestId("sub-dot")).toHaveAttribute("data-stale", "true");
  });

  it("offers the settings entry instead of numbers when no credential is linked", () => {
    const onOpenSettings = vi.fn();
    render(
      <SubscriptionStatus
        status={data({
          state: "unknown",
          windows: [],
          worstPercent: null,
          credential: null,
          errorKind: "no_credential",
        })}
        onOpenSettings={onOpenSettings}
      />,
    );
    open();
    expect(screen.getByRole("tooltip")).toHaveTextContent("未链接凭据");
    expect(screen.queryByTestId("sub-row-rolling")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /设置.*订阅/ }));
    expect(onOpenSettings).toHaveBeenCalledTimes(1);
  });

  it("separates a rejected credential from a failed fetch", () => {
    const { rerender } = render(
      <SubscriptionStatus
        status={data({ state: "invalid", errorKind: "unauthorized", credential: null })}
      />,
    );
    open();
    expect(screen.getByTestId("sub-dot")).toHaveAttribute("data-state", "invalid");
    expect(screen.getByRole("tooltip")).toHaveTextContent(/凭据.*无效/);

    rerender(
      <SubscriptionStatus
        status={data({ state: "error", errorKind: "network", error: "socket closed" })}
      />,
    );
    expect(screen.getByTestId("sub-dot")).toHaveAttribute("data-state", "error");
    expect(screen.getByRole("tooltip")).toHaveTextContent("socket closed");
    // A failed probe is not a missing credential: no "未链接凭据" copy, no linking CTA.
    expect(screen.getByRole("tooltip")).not.toHaveTextContent("未链接凭据");
    expect(screen.queryByRole("button", { name: /设置.*订阅/ })).not.toBeInTheDocument();
  });

  it("opens from the keyboard and closes on blur", () => {
    render(<SubscriptionStatus status={data()} />);
    const trigger = screen.getByTestId("sub-trigger");
    fireEvent.focus(trigger);
    expect(screen.getByRole("tooltip")).toBeInTheDocument();
    fireEvent.blur(trigger);
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  });

  it("keeps the settings entry clickable when focus moves into the panel", () => {
    const onOpenSettings = vi.fn();
    render(
      <SubscriptionStatus
        status={data({
          state: "unknown",
          windows: [],
          worstPercent: null,
          credential: null,
          errorKind: "no_credential",
        })}
        onOpenSettings={onOpenSettings}
      />,
    );
    const trigger = screen.getByTestId("sub-trigger");
    fireEvent.focus(trigger);
    const cta = screen.getByRole("button", { name: /设置.*订阅/ });
    // Tabbing (or mousedown-ing) into the panel blurs the trigger first; the panel must
    // survive that so the button is still there when the click lands.
    fireEvent.blur(trigger, { relatedTarget: cta });
    expect(screen.getByRole("tooltip")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /设置.*订阅/ }));
    expect(onOpenSettings).toHaveBeenCalledTimes(1);
  });

  it("keeps the last numbers visible while reporting a fetch failure", () => {
    render(
      <SubscriptionStatus
        status={data({ state: "error", errorKind: "network", error: "socket closed", windows: [win("rolling", 40)] })}
      />,
    );
    open();
    expect(screen.getByTestId("sub-row-rolling")).toHaveTextContent("40%");
    expect(screen.getByRole("tooltip")).toHaveTextContent("socket closed");
  });
});
