import { describe, expect, test } from "bun:test";
import { startMockKernel, driveRun } from "./mock-kernel.ts";

describe("mock kernel e2e", () => {
  test("full run flow against mock kernel", async () => {
    const k = await startMockKernel();
    const events = await driveRun(k.url, "hi", { approve: "allow" });
    expect(events).toEqual(["run", "progress", "permission", "respond", "done"]);
    await k.close();
  }, 15000);

  test("run completes with usage progress", async () => {
    const k = await startMockKernel();
    const events = await driveRun(k.url, "hello", { approve: "allow" });
    expect(events).toContain("done");
    expect(events[0]).toBe("run");
    expect(events).toContain("progress");
    await k.close();
  }, 15000);
});
