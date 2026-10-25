import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { RpcClient } from "@agentj/sdk";
import { useFileTree } from "./useFileTree";

const entry = (path: string, kind: "file" | "dir" = "file") => ({
  name: path.split("/").pop(),
  path,
  kind,
  git_status: null,
});

/** Client double whose replies are resolved by hand, one request at a time. */
function pendingClient() {
  const pending: Array<(reply: unknown) => void> = [];
  const request = vi.fn(
    (_method: string, _params: Record<string, unknown> = {}) =>
      new Promise((resolve) => {
        pending.push(resolve);
      }),
  );
  return { client: { request } as unknown as RpcClient, pending, request };
}

describe("useFileTree", () => {
  it("discards a reply that belongs to the previous session", async () => {
    const { client, pending } = pendingClient();
    const { result, rerender } = renderHook((props) => useFileTree(props), {
      initialProps: { client, sessionId: "s1" as string | null, demo: false },
    });
    expect(pending).toHaveLength(1);

    rerender({ client, sessionId: "s2", demo: false });
    expect(pending).toHaveLength(2);

    // The s1 reply lands after the switch: it must not paint into the s2 tree.
    await act(async () => {
      pending[0]({ entries: [entry("stale.rs")], truncated: false });
    });
    await waitFor(() => expect(result.current.paths).not.toContain("stale.rs"));

    await act(async () => {
      pending[1]({ entries: [entry("src", "dir")], truncated: false });
    });
    await waitFor(() => expect(result.current.paths).toEqual(["src"]));
    expect(result.current.stats).toEqual({ files: 0, changes: 0 });
  });

  it("loads the demo fixture without a request and counts its rows", () => {
    const { client, request } = pendingClient();
    const { result } = renderHook(() => useFileTree({ client, sessionId: "s1", demo: true }));
    expect(request).not.toHaveBeenCalled();
    expect(result.current.paths.length).toBeGreaterThan(0);
    expect(result.current.stats.files).toBeGreaterThan(0);
    expect(result.current.rootEntries?.length).toBeGreaterThan(0);
  });

  it("toggles a directory once and reuses the cached children", async () => {
    const { client, pending, request } = pendingClient();
    const { result } = renderHook(() => useFileTree({ client, sessionId: "s1", demo: false }));
    await act(async () => {
      pending[0]({ entries: [entry("src", "dir")], truncated: false });
    });
    await waitFor(() => expect(result.current.paths).toEqual(["src"]));

    await act(async () => {
      result.current.toggle("src");
    });
    expect(pending).toHaveLength(2);
    await act(async () => {
      pending[1]({ entries: [entry("src/main.rs")], truncated: false });
    });
    await waitFor(() => expect(result.current.paths).toEqual(["src", "src/main.rs"]));

    await act(async () => {
      result.current.toggle("src");
    });
    await act(async () => {
      result.current.toggle("src");
    });
    await waitFor(() => expect(result.current.paths).toEqual(["src", "src/main.rs"]));
    expect(request).toHaveBeenCalledTimes(2);
  });
});
