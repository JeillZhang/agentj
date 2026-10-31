import "@testing-library/jest-dom/vitest";
import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import SubscriptionCredentialCard from "./SubscriptionCredentialCard";

const base = {
  auth: { authenticated: true, credential: { kind: "api_key", key_hint: "sk-…1234" } },
  apiKey: "",
  setKey: () => {},
  envShadow: false,
  linkMsg: null,
  device: { user_code: "UC-1", verification_uri: "https://example.test/device" },
  deviceMsg: "超时",
  onLink: () => {},
  onClear: () => {},
  onStartDevice: () => {},
};

describe("SubscriptionCredentialCard", () => {
  it("renders the link, device and clear controls", () => {
    const onStartDevice = vi.fn();
    const onClear = vi.fn();
    render(<SubscriptionCredentialCard {...base} onStartDevice={onStartDevice} onClear={onClear} />);
    expect(screen.getByLabelText("API Key")).toBeTruthy();
    fireEvent.click(screen.getByText("设备码登录"));
    expect(onStartDevice).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByText("解除链接"));
    expect(onClear).toHaveBeenCalledTimes(1);
    expect(screen.getByText("UC-1")).toBeTruthy();
    expect(screen.getByText("超时")).toBeTruthy();
  });
});
