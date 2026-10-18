// The ONLY place the fixture path is computed; every other module imports from here.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

export type FixtureEntry = { name: string; path: string; kind: "file" | "directory" };
// Fixture-local vocabulary: `"directory"` here, but the wire enum spells it `"dir"` —
// translate via `fixtureKindToWire` before sending a reply.
export type RankCase = {
  name: string;
  path: string;
  query: string;
  expected_rank: 0 | 1 | 2 | null;
};
export type SearchCase = {
  query: string;
  entries: FixtureEntry[];
  expected_ranked_paths: string[];
  expected_total: number;
};
export type SearchRankCases = {
  version: number;
  rule: string;
  ranks: RankCase[];
  searches: SearchCase[];
};

const EXPECTED_VERSION = 1;

/** `agentj-ui/apps/tui/src/` -> repo root is four levels up. */
export function searchRankCasesPath(): string {
  return fileURLToPath(new URL("../../../../protocol/search-rank-cases.json", import.meta.url));
}

export function parseRankCases(raw: string, path: string): SearchRankCases {
  let parsed: SearchRankCases;
  try {
    parsed = JSON.parse(raw) as SearchRankCases;
  } catch (cause) {
    throw new Error(`search-rank conformance fixture unparseable at ${path}: ${String(cause)}`);
  }
  if (parsed.version !== EXPECTED_VERSION) {
    throw new Error(
      `search-rank conformance fixture ${path} has version ${parsed.version}, expected ${EXPECTED_VERSION}`,
    );
  }
  if (!Array.isArray(parsed.ranks) || !Array.isArray(parsed.searches)) {
    const family = Array.isArray(parsed.ranks) ? "searches" : "ranks";
    throw new Error(
      `search-rank conformance fixture ${path} has a non-array ${family} (expected arrays)`,
    );
  }
  return parsed;
}

function load(): SearchRankCases {
  const path = searchRankCasesPath();
  let raw: string;
  try {
    raw = readFileSync(path, "utf8");
  } catch (cause) {
    throw new Error(`search-rank conformance fixture unreadable at ${path}: ${String(cause)}`);
  }
  return parseRankCases(raw, path);
}

// Fail-closed BY DESIGN: imported only by tests and the e2e mock kernel (never the
// production UI), so a missing fixture fails loudly at import instead of silently.
export const SEARCH_RANK_CASES: SearchRankCases = load();
