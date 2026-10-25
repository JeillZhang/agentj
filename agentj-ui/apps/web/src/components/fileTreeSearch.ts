// Pure guard for server-side file search replies. No React, no timers: the caller passes the
// identity captured when the request went out and the identity current at commit time.

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
