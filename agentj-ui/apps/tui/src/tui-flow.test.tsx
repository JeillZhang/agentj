import { describe, expect, test, mock } from "bun:test";
import { testRender } from "@opentui/react/test-utils";
import type { TestRendererSetup } from "@opentui/core/testing";
import { act } from "react";
import { Composer } from "./composer.tsx";
import { RunView, useProgressFeed } from "./runview.tsx";
import { METHODS } from "@agentj/sdk";
import type { RpcClient } from "@agentj/sdk";

function getInput(setup: TestRendererSetup) {
  const input = setup.mockInput as unknown as {
    press?: (k: string) => void;
    type?: (s: string) => Promise<void>;
  };
  if (!input.press) {
    input.press = (k: string) => {
      if (k === "down") setup.mockInput.pressArrow("down");
      else if (k === "up") setup.mockInput.pressArrow("up");
      else if (k === "enter" || k === "return") setup.mockInput.pressEnter();
      else if (k === "esc" || k === "escape") setup.mockInput.pressEscape();
    };
  }
  if (!input.type) {
    input.type = async (s: string) => {
      await setup.mockInput.typeText(s);
    };
  }
  return input as { press: (k: string) => void; type: (s: string) => Promise<void> };
}

describe("Composer", () => {
  test("composer submits prompt text", async () => {
    const onSubmit = mock(() => {});
    const setup = await testRender(<Composer onSubmit={onSubmit} />, {});
    const input = getInput(setup);
    await input.type("hi");
    await setup.flush();
    act(() => input.press("enter"));
    await setup.flush();
    await setup.waitFor(() => onSubmit.mock.calls.length > 0).catch(() => {});
    expect(onSubmit).toHaveBeenCalledWith("hi");
    act(() => setup.renderer.destroy());
  });

  test("composer clears after submit", async () => {
    const onSubmit = mock(() => {});
    const setup = await testRender(<Composer onSubmit={onSubmit} />, {});
    const input = getInput(setup);
    await input.type("hi");
    await setup.flush();
    act(() => input.press("enter"));
    await setup.flush();
    await new Promise((r) => setTimeout(r, 20));
    await setup.flush();
    expect(setup.captureCharFrame()).not.toContain("hi");
    act(() => setup.renderer.destroy());
  });

  test("composer history up/down", async () => {
    const onSubmit = mock(() => {});
    const setup = await testRender(<Composer onSubmit={onSubmit} />, {});
    const input = getInput(setup);
    await input.type("first");
    await setup.flush();
    act(() => input.press("enter"));
    await setup.flush();
    await new Promise((r) => setTimeout(r, 20));
    await setup.flush();
    await input.type("second");
    await setup.flush();
    act(() => input.press("enter"));
    await setup.flush();
    await new Promise((r) => setTimeout(r, 20));
    await setup.flush();
    act(() => input.press("up"));
    await setup.flush();
    await new Promise((r) => setTimeout(r, 20));
    await setup.flush();
    expect(setup.captureCharFrame()).toContain("second");
    act(() => input.press("up"));
    await setup.flush();
    await new Promise((r) => setTimeout(r, 20));
    await setup.flush();
    expect(setup.captureCharFrame()).toContain("first");
    act(() => input.press("down"));
    await setup.flush();
    await new Promise((r) => setTimeout(r, 20));
    await setup.flush();
    expect(setup.captureCharFrame()).toContain("second");
    act(() => setup.renderer.destroy());
  });

  test("composer clears on Esc", async () => {
    const onSubmit = mock(() => {});
    const setup = await testRender(<Composer onSubmit={onSubmit} />, {});
    const input = getInput(setup);
    await input.type("hello");
    await setup.flush();
    act(() => input.press("escape"));
    await setup.flush();
    await new Promise((r) => setTimeout(r, 30));
    await setup.flush();
    const frame = setup.captureCharFrame();
    expect(frame).not.toContain("hello");
    act(() => setup.renderer.destroy());
  });
});

describe("RunView", () => {
  test("runview appends progress frames in order", async () => {
    const setup = await testRender(<RunView frames={["a"]} />, {});
    await setup.flush();
    let frame = setup.captureCharFrame();
    expect(frame).toContain("a");
    act(() => setup.renderer.destroy());
    const setup2 = await testRender(<RunView frames={["a", "b"]} />, {});
    await setup2.flush();
    frame = setup2.captureCharFrame();
    expect(frame).toContain("a");
    expect(frame).toContain("b");
    act(() => setup2.renderer.destroy());
  });

  test("runview append without dropping prior rows", async () => {
    const setup = await testRender(<RunView frames={["first"]} />, {});
    await setup.flush();
    expect(setup.captureCharFrame()).toContain("first");
    act(() => setup.renderer.destroy());
    const setup2 = await testRender(<RunView frames={["first", "second", "third"]} />, {});
    await setup2.flush();
    const frame = setup2.captureCharFrame();
    expect(frame).toContain("first");
    expect(frame).toContain("second");
    expect(frame).toContain("third");
    act(() => setup2.renderer.destroy());
  });
});

describe("Progress feed via METHODS.progress", () => {
  test("useProgressFeed filters METHODS.progress", async () => {
    const frames: string[] = [];
    let handler: ((m: string, p: unknown) => void) | null = null;
    const fake = {
      setNotificationHandler(h: (m: string, p: unknown) => void) {
        handler = h;
      },
    } as unknown as RpcClient;
    function Probe() {
      useProgressFeed(fake, (t) => frames.push(t));
      return <text>probe</text>;
    }
    const setup = await testRender(<Probe />, {});
    await setup.flush();
    handler?.(METHODS.progress, { text: "hello" });
    handler?.("other/method", { text: "ignore" });
    handler?.(METHODS.progress, { text: "world" });
    await setup.flush();
    expect(frames).toEqual(["hello", "world"]);
    act(() => setup.renderer.destroy());
  });

  test("composer submit triggers session/run", async () => {
    const calls: Array<{ method: string; params: Record<string, unknown> }> = [];
    const fake = {
      request: async (m: string, p: Record<string, unknown>) => {
        calls.push({ method: m, params: p });
        return {};
      },
    } as unknown as RpcClient;
    function Wrapped() {
      const onSubmit = (text: string) => {
        void fake.request(METHODS.sessionRun, { session_id: "s1", prompt: text });
      };
      return <Composer onSubmit={onSubmit} />;
    }
    const setup = await testRender(<Wrapped />, {});
    const input = getInput(setup);
    await input.type("hi");
    await setup.flush();
    act(() => input.press("enter"));
    await setup.flush();
    await setup.waitFor(() => calls.length > 0).catch(() => {});
    expect(calls[0]?.method).toBe(METHODS.sessionRun);
    expect(calls[0]?.params.prompt).toBe("hi");
    expect(calls[0]?.params.session_id).toBe("s1");
    act(() => setup.renderer.destroy());
  });
});
