import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { METHODS } from "@agentj/sdk";
import type { RpcClient } from "@agentj/sdk";
import { useFileSearch } from "./useFileSearch";

afterEach(() => {
  vi.useRealTimers();
});

/** Client double that records every search query it was asked for. */
function searchClient(hits: unknown = { entries: [], total: 0, truncated: false }) {
  const request = vi.fn(async (method: string, params: Record<string, unknown> = {}) => {
    if (method === METHODS.fileList && "query" in params) return hits;
    return { entries: [], truncated: false };
  });
  return { client: { request } as unknown as RpcClient, request };
}

/** Queries that actually reached the kernel (the debounce may swallow the rest). */
const sentQueries = (request: ReturnType<typeof searchClient>["request"]): unknown[] =>
  request.mock.calls
    .filter(([, params]) => "query" in ((params ?? {}) as object))
    .map(([, params]) => (params as Record<string, unknown>).query);

describe("useFileSearch", () => {
  it("does not request anything when the session changed inside the debounce window", async () => {
    vi.useFakeTimers();
    const { client, request } = searchClient();
    const { result, rerender } = renderHook((props) => useFileSearch(props), {
      initialProps: { client, sessionId: "s1" as string | null, demo: false },
    });

    act(() => {
      result.current.changeQuery("ma");
    });
    rerender({ client, sessionId: "s2", demo: false });
    expect(result.current.state.query).toBe("");

    await act(async () => {
      vi.advanceTimersByTime(200);
    });
    expect(sentQueries(request)).toEqual([]);
  });

  it("debounces a burst of keystrokes into one request and applies the reply", async () => {
    vi.useFakeTimers();
    const hits = {
      entries: [{ name: "main.ts", path: "src/main.ts", kind: "file", git_status: "M" }],
      total: 3,
      truncated: false,
    };
    const { client, request } = searchClient(hits);
    const { result } = renderHook(() => useFileSearch({ client, sessionId: "s1", demo: false }));

    act(() => {
      result.current.changeQuery("ma");
      result.current.changeQuery("mai");
      result.current.changeQuery("main");
    });
    expect(sentQueries(request)).toEqual([]);
    await act(async () => {
      vi.advanceTimersByTime(150);
    });

    expect(sentQueries(request)).toEqual(["main"]);
    expect(request.mock.calls.at(-1)?.[1]).toEqual({ session_id: "s1", path: null, query: "main" });
    expect(result.current.state.rows.map((row) => row.path)).toEqual(["src/main.ts"]);
    expect(result.current.state.total).toBe(3);
    expect(result.current.active).toBe(true);
  });

  it("exitSearch cancels a pending request and returns to the tree", async () => {
    vi.useFakeTimers();
    const { client, request } = searchClient();
    const { result } = renderHook(() => useFileSearch({ client, sessionId: "s1", demo: false }));

    act(() => {
      result.current.changeQuery("main");
    });
    act(() => {
      result.current.exitSearch();
    });
    await act(async () => {
      vi.advanceTimersByTime(200);
    });

    expect(sentQueries(request)).toEqual([]);
    expect(result.current.active).toBe(false);
    expect(result.current.state.query).toBe("");
  });

  it("never searches in demo mode", () => {
    const { client, request } = searchClient();
    const { result } = renderHook(() => useFileSearch({ client, sessionId: "s1", demo: true }));
    act(() => {
      result.current.changeQuery("main");
    });
    expect(result.current.active).toBe(false);
    expect(request).not.toHaveBeenCalled();
  });
});
