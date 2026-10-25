import { describe, expect, it } from "vitest";
import { deviceStateLabel } from "./subscriptionModel";


describe("deviceStateLabel", () => {
  it("labels each terminal poll outcome", () => {
    expect(deviceStateLabel("authorized")).toBe("登录成功");
    expect(deviceStateLabel("expired")).toBe("设备码已过期，请重新登录");
    expect(deviceStateLabel("denied")).toBe("设备码登录被拒绝");
  });

  it("stays silent while the poll should continue", () => {
    expect(deviceStateLabel("pending")).toBeNull();
    expect(deviceStateLabel("slow_down")).toBeNull();
  });

  it("falls back for an unknown state instead of hanging on a blank panel", () => {
    expect(deviceStateLabel("something-new")).toBe("设备码登录未完成，请重试");
  });
});
