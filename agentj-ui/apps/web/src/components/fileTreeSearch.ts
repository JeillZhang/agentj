// Pure helpers for server-side file search: the reply guard, the wire normalization and the
// reply → view-state reducer. No React, no timers: the caller passes the identity captured
// when the request went out and the identity current at commit time.

import { buildSearchIndex } from "@agentj/sdk";
import type { RawFileEntry as SharedRawFileEntry, SearchEntry } from "@agentj/sdk";
import { normalizeEntry } from "./fileTreeModel";
import type { FileListReply, RawFileEntry } from "./fileTreeModel";

/** Monotonic request identity: sequence number plus the session the request belongs to. */
export type SearchGuard = {
  seq: number;
  session: string | null;
};

/**
 * True while a reply is still the latest request AND belongs to the session that is current
 * when the reply is committed. The session half is read from a ref refreshed before paint, so
 * a reply resolving in the microtask window after a session switch is dropped instead of
 * painting the previous session's rows for one frame.
 */
export function replyIsCurrent(request: SearchGuard, current: SearchGuard): boolean {
  return request.seq === current.seq && request.session === current.session;
}

/** Wait before asking the kernel so a fast typist issues one search, not one per key. */
export const SEARCH_DEBOUNCE_MS = 150;

/** Shown when the reply carries no `total`: the kernel predates `file/list { query }`. */
export const SEARCH_UNSUPPORTED_HINT = "内核不支持服务端搜索";

/**
 * Search is a separate mode, not a filter over the loaded map: the kernel ranks
 * (`file_list::search_rank`) and this component only renders what came back.
 */
export type SearchState = {
  active: boolean;
  query: string;
  rows: SearchEntry[];
  total: number;
  truncated: boolean;
  /** A request for the current query is scheduled or in flight; counts are not final yet. */
  loading: boolean;
  error: string | null;
  /** Old-kernel notice; mutually exclusive with `error`. */
  notice: string | null;
};

export const IDLE_SEARCH: SearchState = {
  active: false,
  query: "",
  rows: [],
  total: 0,
  truncated: false,
  loading: false,
  error: null,
  notice: null,
};

/** Narrows one wire entry into the shared search-entry shape via [`normalizeEntry`]. */
export function toSearchEntry(entry: RawFileEntry): SharedRawFileEntry {
  const { git, ...rest } = normalizeEntry(entry);
  return { ...rest, git_status: git };
}

/**
 * A reply carrying `total` is a server-ranked search. Without it the kernel cannot
 * search at all — say so instead of filtering the loaded tree locally, which would
 * report a subset of a subset as if it were the whole answer.
 */
export function applySearchReply(prev: SearchState, reply: FileListReply): SearchState {
  if (typeof reply?.total !== "number") {
    return {
      ...prev,
      rows: [],
      total: 0,
      truncated: false,
      loading: false,
      error: null,
      notice: SEARCH_UNSUPPORTED_HINT,
    };
  }
  return {
    ...prev,
    rows: buildSearchIndex((reply.entries ?? []).map(toSearchEntry)),
    total: reply.total,
    truncated: Boolean(reply.truncated),
    loading: false,
    error: null,
    notice: null,
  };
}

/** Header line above the result rows. */
export function searchHead(state: SearchState): string {
  if (state.notice) return state.notice;
  if (state.loading) return "搜索中…";
  if (state.query.trim().length === 0) return "输入关键字搜索文件名";
  return `搜索到 ${state.rows.length} / ${state.total} 项${state.truncated ? " · 部分结果" : ""}`;
}
