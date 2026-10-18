// The sidebar owns its own copy of the session list, so it has to be told when
// the server-side list changed (a run settled => preview/updated_at moved, or a
// session was created/removed elsewhere). A window event keeps the two panels
// decoupled: neither imports the other.

export const SESSIONS_UPDATED_EVENT = "agentj:sessions-updated";

/** Ask every mounted session list to refetch. */
export function notifySessionsUpdated(): void {
  window.dispatchEvent(new CustomEvent(SESSIONS_UPDATED_EVENT));
}

/** Subscribe to refetch requests; returns the unsubscribe function. */
export function subscribeSessionsUpdated(handler: () => void): () => void {
  window.addEventListener(SESSIONS_UPDATED_EVENT, handler as EventListener);
  return () => window.removeEventListener(SESSIONS_UPDATED_EVENT, handler as EventListener);
}
