// agentj-ui/packages/sdk/src/__tests__/toolSummary.test.ts
// Ported from agentj-ui/apps/web/src/RunPanel.test.tsx (the Web chip assertions).
import { describe, expect, it } from "vitest";
import { toolSummary } from "../toolSummary.ts";

describe("toolSummary", () => {
  it("summarises each builtin tool", () => {
    expect(toolSummary({ name: "bash", args: { command: "ls -la" } })).toBe("ls -la");
    expect(toolSummary({ name: "read", args: { path: "src/a.ts" } })).toBe("src/a.ts");
    expect(toolSummary({ name: "write", args: { path: "src/a.ts" } })).toBe("src/a.ts");
    expect(toolSummary({ name: "glob", args: { pattern: "**/*.rs" } })).toBe("**/*.rs");
    expect(toolSummary({ name: "grep", args: { pattern: "fn", include: "*.rs" } })).toBe("fn · *.rs");
    expect(toolSummary({ name: "grep", args: { pattern: "fn", include: "*" } })).toBe("fn");
    expect(toolSummary({ name: "grep", args: { pattern: "fn" } })).toBe("fn");
    expect(toolSummary({ name: "todo", args: { items: [1, 2] } })).toBe("2 项");
  });

  it("returns an empty string for malformed input", () => {
    expect(toolSummary({ name: "bash", args: null })).toBe("");
    expect(toolSummary({ name: "bash", args: 7 })).toBe("");
    expect(toolSummary({ name: "bash", args: [1, 2] })).toBe("");
    expect(toolSummary({ name: "todo", args: { items: "nope" } })).toBe("");
    expect(toolSummary({ name: "unknown", args: { x: 1 } })).toBe("");
  });

  it("ignores non-string arg values", () => {
    expect(toolSummary({ name: "bash", args: { command: 42 } })).toBe("");
  });
});
