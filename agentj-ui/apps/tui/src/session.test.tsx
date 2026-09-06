import { describe, expect, test, mock } from "bun:test";
import { testRender } from "@opentui/react/test-utils";
import { act } from "react";
import { useEffect } from "react";
import { METHODS } from "@agentj/sdk";
import type { RpcClient } from "@agentj/sdk";
import { SessionList, addToHistory, useSession } from "./session.tsx";

describe("SessionList", () => {
  test("session list selects and switches", async () => {
    const onSelect = mock(() => {});
    const setup = await testRender(
      <SessionList history={["s1", "s2"]} onSelect={onSelect} />,
      {},
    );
    const mockInput = setup.mockInput;
    type PressShim = typeof mockInput & { press?: (k: string) => void };
    // library shim – map plan's input.press to real mockInput API
    const input = mockInput as unknown as PressShim;
    if (!input.press) {
      input.press = (k: string) => {
        if (k === "down") mockInput.pressArrow("down");
        else if (k === "up") mockInput.pressArrow("up");
        else if (k === "enter" || k === "return") mockInput.pressEnter();
      };
    }
    await act(async () => {
      input.press?.("down");
    });
    await act(async () => {
      await setup.flush();
    });
    await act(async () => {
      input.press?.("enter");
    });
    await act(async () => {
      await setup.flush();
    });
    await act(async () => {
      await setup.waitFor(() => onSelect.mock.calls.length > 0).catch(() => {});
    });
    expect(onSelect).toHaveBeenCalledWith("s2");
    act(() => setup.renderer.destroy());
  });

  test("addToHistory puts active first and dedupes", () => {
    const next = addToHistory(["s1", "s2"], "s2");
    expect(next[0]).toBe("s2");
    expect(next).toEqual(["s2", "s1"]);
  });
});

describe("useSession", () => {
  test("createNew calls session/new via METHODS", async () => {
    const calls: string[] = [];
    const fake = {
      request: async (m: string) => {
        calls.push(m);
        return { session_id: "s9" };
      },
    } as unknown as RpcClient;
    function Probe(props: { done: (id: string) => void }) {
      const { createNew } = useSession(fake);
      useEffect(() => {
        createNew().then(props.done);
      }, []);
      return null;
    }
    let result = "";
    const setup = await testRender(<Probe done={(id) => (result = id)} />, {});
    await act(async () => {
      await setup.waitFor(() => result === "s9");
    });
    expect(calls).toContain(METHODS.sessionNew);
    expect(result).toBe("s9");
    act(() => setup.renderer.destroy());
  });

  test("continueSession calls status and subscribe", async () => {
    const calls: string[] = [];
    const fake = {
      request: async (m: string) => {
        calls.push(m);
        return {};
      },
    } as unknown as RpcClient;
    function Probe(props: { done: (v: string) => void }) {
      const { continueSession } = useSession(fake);
      useEffect(() => {
        continueSession("s2").then(props.done);
      }, []);
      return null;
    }
    let done = "";
    const setup = await testRender(<Probe done={(v) => (done = v)} />, {});
    await act(async () => {
      await setup.waitFor(() => done === "s2");
    });
    expect(calls).toContain(METHODS.sessionStatus);
    expect(calls).toContain(METHODS.sessionSubscribe);
    act(() => setup.renderer.destroy());
  });
});
