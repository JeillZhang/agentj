import "@testing-library/jest-dom/vitest";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import SessionPanel, { rowsFromList, type ListRow } from "./SessionPanel.tsx";
import { METHODS } from "@agentj/sdk";

const calls: Array<{ method: string; params: Record<string, unknown> }> = [];
const listHandlers: Record<string, (params: unknown) => unknown> = {};

vi.mock("@agentj/sdk", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@agentj/sdk")>();
  class FakeRpcClient {
    request = vi.fn(async (method: string, params: unknown = {}) => {
      calls.push({ method, params: params as Record<string, unknown> });
      const h = listHandlers[method];
      return h ? h(params) : {};
    });
    setNotificationHandler = vi.fn();
  }
  return {
        ...actual,
        ResponseRouter: class {},
        WebSocketTransport: class { close() {} },
        RpcClient: FakeRpcClient,
      };
});

describe("rowsFromList", () => {
  it("maps rows in list order and truncates previews at 120 chars", () => {
    const long = "x".repeat(200);
    const rows: ListRow[] = rowsFromList([
      { session_id: "s1", state: "active", preview: "hello" },
      { session_id: "s2", state: "archived", preview: long },
      { session_id: "s3", state: "active", preview: null },
    ]);
    expect(rows.map((r) => r.id)).toEqual(["s1", "s2", "s3"]);
    expect(rows[1].preview!.length).toBeLessThanOrEqual(120);
    expect(rows[1].preview).toMatch(/…$/);
    expect(rows[2].preview).toBeUndefined();
  });
});

describe("SessionPanel session list", () => {
  beforeEach(() => {
    calls.length = 0;
    delete listHandlers[METHODS.sessionList];
    listHandlers[METHODS.sessionList] = () => ({
      sessions: [
        { session_id: "s1", state: "active", preview: "hi" },
        { session_id: "s2", state: "archived", preview: "old" },
      ],
    });
  });
  function renderPanel(onActiveChange: () => void = () => {}) {
    return render(
      <SessionPanel wsUrl="ws://127.0.0.1:1" activeSessionId={null} onActiveChange={onActiveChange} />,
    );
  }
  it("loads session/list once and renders the rows", async () => {
    renderPanel();
    const list = await screen.findByRole("list", { name: "session list" });
    expect(await within(list).findByText(/s1/)).toBeInTheDocument();
    expect(within(list).getByText(/s2/)).toBeInTheDocument();
    await waitFor(() => {
      expect(calls.filter((c) => c.method === METHODS.sessionList)).toHaveLength(1);
    });
  });
  it("activates a clicked row through status + subscribe", async () => {
    const onActiveChange = vi.fn();
    renderPanel(onActiveChange);
    const list = await screen.findByRole("list", { name: "session list" });
    fireEvent.click(await within(list).findByText(/s1/));
    await waitFor(() => {
      expect(calls.map((c) => c.method)).toEqual([
        METHODS.sessionList,
        METHODS.sessionStatus,
        METHODS.sessionSubscribe,
      ]);
      expect(onActiveChange).toHaveBeenCalledWith("s1");
    });
  });
  it("typed continue never re-fetches session/list", async () => {
    renderPanel();
    fireEvent.change(await screen.findByPlaceholderText(/session/i), { target: { value: "s9" } });
    fireEvent.click(screen.getByRole("button", { name: /^continue$/i }));
    await waitFor(() => {
      expect(calls.filter((c) => c.method === METHODS.sessionList)).toHaveLength(1);
      expect(calls.slice(-2).map((c) => c.method)).toEqual([
        METHODS.sessionStatus,
        METHODS.sessionSubscribe,
      ]);
    });
  });
});
