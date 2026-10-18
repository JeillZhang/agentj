// agentj-ui/apps/tui/src/searchRankCases.test.ts — CREATE
import { describe, expect, test } from "bun:test";
import { existsSync } from "node:fs";
import { SEARCH_RANK_CASES, parseRankCases, searchRankCasesPath } from "./searchRankCases";

describe("search rank conformance fixture loader", () => {
  test("loads version 1 with both case families populated", () => {
    expect(SEARCH_RANK_CASES.version).toBe(1);
    expect(SEARCH_RANK_CASES.ranks.length).toBeGreaterThan(0);
    expect(SEARCH_RANK_CASES.searches.length).toBeGreaterThan(0);
  });

  test("resolves the fixture relative to the module, not the process cwd", () => {
    const path = searchRankCasesPath();
    expect(path.endsWith("/protocol/search-rank-cases.json")).toBe(true);
    expect(existsSync(path)).toBe(true);
  });

  test("pins all four rank outcomes", () => {
    const seen = [...new Set(SEARCH_RANK_CASES.ranks.map((c) => c.expected_rank))];
    // Explicit comparator: the default sort stringifies, which would order null last.
    expect(seen.sort((a, b) => (a ?? 3) - (b ?? 3))).toEqual([0, 1, 2, null]);
  });

  test("rejects a fixture whose version is not the expected one", () => {
    const raw = JSON.stringify({ version: 2, rule: "r", ranks: [], searches: [] });
    expect(() => parseRankCases(raw, "/tmp/search-rank-cases.json")).toThrow(
      /\/tmp\/search-rank-cases\.json.*version 2.*expected 1/,
    );
  });

  test("rejects a fixture whose case families are not arrays", () => {
    const raw = JSON.stringify({ version: 1, rule: "r", ranks: {}, searches: [] });
    expect(() => parseRankCases(raw, "/tmp/search-rank-cases.json")).toThrow(
      /\/tmp\/search-rank-cases\.json.*ranks.*array/,
    );
  });
});
