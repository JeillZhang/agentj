// fileTreeSearch.test.ts — the reply guard that keeps stale search results off screen.
import { expect, test } from "vitest";
import { replyIsCurrent } from "./fileTreeSearch";

test("accepts a reply from the latest request of the current session", () => {
  expect(replyIsCurrent({ seq: 7, session: "s1" }, { seq: 7, session: "s1" })).toBe(true);
});

test("rejects a reply whose request was superseded", () => {
  expect(replyIsCurrent({ seq: 6, session: "s1" }, { seq: 7, session: "s1" })).toBe(false);
});

test("rejects a reply from another session even when seq and query still match", () => {
  // Same seq happens on a clean session switch: nothing else distinguishes the two requests.
  expect(replyIsCurrent({ seq: 7, session: "s1" }, { seq: 7, session: "s2" })).toBe(false);
  // A pending session (no owner yet) is not the same as a real one either.
  expect(replyIsCurrent({ seq: 7, session: null }, { seq: 7, session: "s1" })).toBe(false);
});
