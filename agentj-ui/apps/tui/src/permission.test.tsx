import { describe, expect, test, mock } from "bun:test";
import { testRender } from "@opentui/react/test-utils";
import type { TestRendererSetup } from "@opentui/core/testing";
import { act } from "react";
import { useState } from "react";
import { PermissionDialog, usePermission, usePermissionState } from "./permission.tsx";
import { METHODS } from "@agentj/sdk";
import type { RpcClient } from "@agentj/sdk";

function getInput(setup: TestRendererSetup) {
  const mi = setup.mockInput as unknown as {
    pressTab: (m?: unknown) => void;
    pressEnter: (m?: unknown) => void;
    pressEscape: (m?: unknown) => void;
    pressKey: (k: string, m?: unknown) => void;
    press?: (k: string) => void;
  };
  if (!mi.press) {
    mi.press = (k: string) => {
      if (k === "tab") mi.pressTab();
      else if (k === "enter" || k === "return") mi.pressEnter();
      else if (k === "escape" || k === "esc") mi.pressEscape();
      else mi.pressKey(k as string);
    };
  }
  return mi as { press: (k: string) => void; pressTab: () => void; pressEnter: () => void };
}

describe("PermissionDialog", () => {
  test("shows modal with 3 focusable options + session_id", async () => {
    const onRespond = mock(() => {});
    const setup = await testRender(
      <PermissionDialog
        tool="bash"
        args='{"cmd":"rm -rf /tmp/x"}'
        sessionId="sess-123"
        onRespond={onRespond}
      />,
      {},
    );
    await setup.flush();
    const frame = setup.captureCharFrame();
    expect(frame).toContain("Permission required");
    expect(frame).toContain("bash");
    expect(frame).toContain("sess-123");
    expect(frame).toContain("Allow");
    expect(frame).toContain("Deny");
    expect(frame).toContain("AllowAll");
    act(() => setup.renderer.destroy());
  });

  test("displays args and session_id line", async () => {
    const onRespond = mock(() => {});
    const setup = await testRender(
      <PermissionDialog tool="bash" args="rm -rf /tmp/x" session_id="s-9" onRespond={onRespond} />,
      {},
    );
    await setup.flush();
    const frame = setup.captureCharFrame();
    expect(frame).toContain("rm -rf /tmp/x");
    expect(frame).toContain("session: s-9");
    act(() => setup.renderer.destroy());
  });

  test("allow-all responds with allow_always true via tab+enter", async () => {
    const onRespond = mock(() => {});
    const setup = await testRender(
      <PermissionDialog tool="bash" args="rm -rf /tmp/x" onRespond={onRespond} />,
      {},
    );
    const input = getInput(setup);
    await setup.flush();
    act(() => input.press("tab"));
    await setup.flush();
    act(() => input.press("tab"));
    await setup.flush();
    act(() => input.press("enter"));
    await setup.flush();
    await setup.waitFor(() => onRespond.mock.calls.length > 0).catch(() => {});
    expect(onRespond).toHaveBeenCalledWith({ decision: "allow", allow_always: true });
    act(() => setup.renderer.destroy());
  });

  test("allow responds with decision allow", async () => {
    const onRespond = mock(() => {});
    const setup = await testRender(
      <PermissionDialog tool="bash" args="ls" onRespond={onRespond} />,
      {},
    );
    const input = getInput(setup);
    await setup.flush();
    act(() => input.press("enter"));
    await setup.flush();
    await setup.waitFor(() => onRespond.mock.calls.length > 0).catch(() => {});
    const arg = onRespond.mock.calls[0][0] as Record<string, unknown>;
    expect(arg.decision).toBe("allow");
    expect(arg.allow_always).toBeUndefined();
    act(() => setup.renderer.destroy());
  });

  test("deny responds with decision deny via tab+enter", async () => {
    const onRespond = mock(() => {});
    const setup = await testRender(
      <PermissionDialog tool="bash" args="ls" onRespond={onRespond} />,
      {},
    );
    const input = getInput(setup);
    await setup.flush();
    act(() => input.press("tab"));
    await setup.flush();
    act(() => input.press("enter"));
    await setup.flush();
    await setup.waitFor(() => onRespond.mock.calls.length > 0).catch(() => {});
    expect(onRespond).toHaveBeenCalledWith({ decision: "deny" });
    act(() => setup.renderer.destroy());
  });

  test("tab cycles focus", async () => {
    const onRespond = mock(() => {});
    const setup = await testRender(
      <PermissionDialog tool="bash" args="ls" onRespond={onRespond} />,
      {},
    );
    const input = getInput(setup);
    await setup.flush();
    let frame = setup.captureCharFrame();
    expect(frame).toContain("▶ Allow");
    act(() => input.press("tab"));
    await setup.flush();
    frame = setup.captureCharFrame();
    expect(frame).toContain("▶ Deny");
    act(() => input.press("tab"));
    await setup.flush();
    frame = setup.captureCharFrame();
    expect(frame).toContain("▶ AllowAll");
    act(() => input.press("tab"));
    await setup.flush();
    frame = setup.captureCharFrame();
    expect(frame).toContain("▶ Allow");
    act(() => setup.renderer.destroy());
  });
});

describe("Permission notification wiring", () => {
  test("usePermission receives METHODS.permissionRequest", async () => {
    let notify: ((m: string, p: unknown) => void) | null = null;
    const fakeClient = {
      setNotificationHandler(h: (m: string, p: unknown) => void) {
        notify = h;
      },
      request: async () => ({}),
    } as unknown as RpcClient;

    function Host() {
      const [req, setReq] = useState<unknown>(null);
      usePermission(fakeClient, (p) => setReq(p));
      if (req !== null) {
        const text = JSON.stringify(req);
        if (text.includes("bash")) return <text>got:bash</text>;
        return <text>got:other</text>;
      }
      return <text>idle</text>;
    }

    const setup = await testRender(<Host />, {});
    await setup.flush();
    expect(setup.captureCharFrame()).toContain("idle");
    act(() => {
      notify?.(METHODS.permissionRequest, {
        request_id: "r1",
        tool: "bash",
        args: { cmd: "ls" },
        session_id: "s-1",
      });
    });
    await setup.flush();
    await setup.waitFor(() => setup.captureCharFrame().includes("got:bash")).catch(() => {});
    expect(setup.captureCharFrame()).toContain("got:bash");
    act(() => setup.renderer.destroy());
  });

  test("permission flow via App wiring sends allow_always payload", async () => {
    const calls: Array<Record<string, unknown>> = [];
    const handlers: Array<(m: string, p: unknown) => void> = [];
    const fakeClient = {
      setNotificationHandler(h: (m: string, p: unknown) => void) {
        handlers.push(h);
      },
      request: async (_m: string, p: Record<string, unknown>) => {
        calls.push(p);
        return {};
      },
    } as unknown as RpcClient;

    function FlowHost() {
      const { pending, respond, clear } = usePermissionState(fakeClient);
      if (pending) {
        return (
          <PermissionDialog
            tool={pending.tool}
            args={pending.args}
            sessionId={pending.session_id ?? null}
            requestId={pending.request_id}
            onRespond={respond}
            onClose={clear}
          />
        );
      }
      return <text>idle</text>;
    }

    const setup = await testRender(<FlowHost />, {});
    await setup.flush();
    const dispatch = (m: string, p: unknown) => handlers.forEach((h) => h(m, p));
    act(() => {
      dispatch(METHODS.permissionRequest, {
        request_id: "req-9",
        tool: "bash",
        args: { cmd: "rm -rf /tmp/x" },
        session_id: "sess-123",
      });
    });
    await setup.flush();
    await setup.waitFor(() => setup.captureCharFrame().includes("Permission required")).catch(() => {});
    expect(setup.captureCharFrame()).toContain("sess-123");
    expect(setup.captureCharFrame()).toContain("AllowAll");
    const mi = getInput(setup);
    act(() => mi.press("tab"));
    await setup.flush();
    act(() => mi.press("tab"));
    await setup.flush();
    act(() => mi.press("enter"));
    await setup.flush();
    await setup.waitFor(() => calls.length > 0).catch(() => {});
    expect(calls[0].request_id).toBe("req-9");
    expect(calls[0].decision).toBe("allow");
    expect(calls[0].allow_always).toBe(true);
    act(() => setup.renderer.destroy());
  });
});
