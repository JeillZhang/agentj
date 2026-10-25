// useFileSearch.ts — the debounced server-side file search behind the web file tree:
// sequence + session guards, the 150ms debounce and the reply reducer. No rendering.
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { METHODS } from "@agentj/sdk";
import type { RpcClient } from "@agentj/sdk";
import type { FileListReply } from "../components/fileTreeModel";
import {
  IDLE_SEARCH,
  SEARCH_DEBOUNCE_MS,
  applySearchReply,
  replyIsCurrent,
} from "../components/fileTreeSearch";
import type { SearchGuard, SearchState } from "../components/fileTreeSearch";

export type UseFileSearchOptions = {
  client: RpcClient | null;
  sessionId: string | null;
  demo: boolean;
};

export type UseFileSearchResult = {
  state: SearchState;
  /** True while the search surface replaces the tree: the tree keeps its rows untouched. */
  active: boolean;
  changeQuery: (value: string) => void;
  exitSearch: () => void;
};

/**
 * Search is a separate mode, not a filter: the kernel ranks and this hook only holds what
 * came back. A reply is applied while it is still the latest request of the current session.
 */
export function useFileSearch({ client, sessionId, demo }: UseFileSearchOptions): UseFileSearchResult {
  const [state, setState] = useState<SearchState>(IDLE_SEARCH);
  /** Monotonic request id: any reply whose id is stale (query moved on, session changed) is dropped. */
  const searchSeq = useRef(0);
  /**
   * Session every in-flight reply must still belong to. Assigned before paint (layout
   * effect) instead of in the reset effect below: a reply can resolve in the microtask
   * window before passive effects flush and would otherwise paint the previous session's
   * rows for one frame. Microtasks never run inside a commit, so this is always current.
   */
  const sessionRef = useRef(sessionId);
  useLayoutEffect(() => {
    sessionRef.current = sessionId;
  }, [sessionId]);
  const searchTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const cancelSearchTimer = useCallback((): void => {
    if (searchTimer.current === null) return;
    clearTimeout(searchTimer.current);
    searchTimer.current = null;
  }, []);

  // A failed search keeps its rows and error on screen; it is never resent implicitly.
  const runSearch = useCallback(
    async (query: string, seq: number): Promise<void> => {
      // A timer scheduled before a session switch must never reach the new session.
      if (!client || !sessionId || sessionRef.current !== sessionId) return;
      const requestGuard: SearchGuard = { seq, session: sessionRef.current };
      try {
        const reply = (await client.request(METHODS.fileList, {
          session_id: sessionId,
          path: null,
          query,
        })) as FileListReply;
        if (!replyIsCurrent(requestGuard, { seq: searchSeq.current, session: sessionRef.current })) return;
        setState((prev) => (prev.active && prev.query === query ? applySearchReply(prev, reply) : prev));
      } catch (e) {
        if (!replyIsCurrent(requestGuard, { seq: searchSeq.current, session: sessionRef.current })) return;
        const message = e instanceof Error ? e.message : String(e);
        // The rows still on screen belong to the previous query: only the message is current.
        setState((prev) =>
          prev.active && prev.query === query
            ? { ...prev, total: 0, truncated: false, loading: false, error: message, notice: null }
            : prev,
        );
      }
    },
    [client, sessionId],
  );

  const changeQuery = useCallback(
    (value: string): void => {
      // Demo mode never searches: fixtures carry no server-side index.
      if (demo) return;
      cancelSearchTimer();
      searchSeq.current += 1;
      const seq = searchSeq.current;
      const blank = value.trim().length === 0;
      setState((prev) => ({
        ...prev,
        active: true,
        query: value,
        loading: !blank,
        error: null,
        notice: null,
        ...(blank ? { rows: [], total: 0, truncated: false } : {}),
      }));
      if (blank) return;
      searchTimer.current = setTimeout(() => {
        searchTimer.current = null;
        void runSearch(value, seq);
      }, SEARCH_DEBOUNCE_MS);
    },
    [cancelSearchTimer, runSearch, demo],
  );

  const exitSearch = useCallback((): void => {
    cancelSearchTimer();
    searchSeq.current += 1;
    setState(IDLE_SEARCH);
  }, [cancelSearchTimer]);

  useEffect(() => cancelSearchTimer, [cancelSearchTimer]);

  /**
   * A session switch clears the surface so no rows of the previous session survive. Adjusted
   * during render rather than in an effect: the stale rows are then gone in the same commit
   * that changed the session. The pending timer is neutralised instead of cleared here —
   * `runSearch` re-checks the session it was scheduled for before sending, and
   * `applySearchReply` only touches a surface that is still active for the same query.
   */
  const resetKey = `${demo}|${client ? "1" : "0"}|${sessionId ?? ""}`;
  const [appliedResetKey, setAppliedResetKey] = useState(resetKey);
  if (appliedResetKey !== resetKey) {
    setAppliedResetKey(resetKey);
    setState(IDLE_SEARCH);
  }

  return { state, active: state.active, changeQuery, exitSearch };
}
