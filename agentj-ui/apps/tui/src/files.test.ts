// agentj-ui/apps/tui/src/files.test.ts
// The helpers themselves are pinned by @agentj/sdk (packages/sdk/src/__tests__/files.test.ts);
// this file only pins the TUI barrel: it re-exports the shared surface and nothing local.
import { describe, expect, it } from "bun:test";
import * as files from "./files.ts";

describe("files barrel", () => {
  it("re-exports the shared surface", () => {
    expect(Object.keys(files)).toEqual(
      expect.arrayContaining(["buildSearchIndex", "flattenVisible", "parentPath"]),
    );
    expect(Object.keys(files)).not.toEqual(
      expect.arrayContaining(["filterEntries", "searchRank", "comparePaths"]),
    );
  });

  it("imports without side effects", () => {
    expect(typeof files.parentPath).toBe("function");
    expect(typeof files.buildSearchIndex).toBe("function");
  });
});
