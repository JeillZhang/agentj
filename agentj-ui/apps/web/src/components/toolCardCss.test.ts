// agentj-ui/apps/web/src/components/toolCardCss.test.ts
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const css = readFileSync(join(here, "conversation.css"), "utf8");

function rule(selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = css.match(new RegExp(`${escaped}\\s*\\{([^}]*)\\}`));
  return match?.[1] ?? "";
}

describe("tool card full-width layout", () => {
  it("stretches the tool card across the conversation column", () => {
    const card = rule(".aj-tool-card");
    expect(card).toContain("align-self: stretch");
    expect(card).not.toContain("align-self: flex-start");
  });
  it("truncates a long command summary instead of wrapping", () => {
    const summary = rule(".aj-tool-summary");
    expect(summary).toContain("text-overflow: ellipsis");
    expect(summary).toContain("white-space: nowrap");
    expect(summary).toContain("min-width: 0");
  });
});
