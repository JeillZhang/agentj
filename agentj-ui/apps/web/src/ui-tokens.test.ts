import { describe, expect, it } from "vitest";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
const favicon = readFileSync(join(process.cwd(), "public", "favicon.svg"), "utf8");
const PUBLIC = join(process.cwd(), "public");

describe("brand favicon", () => {
  it("is a DeepSeek-blue mark with no brand-purple residue", () => {
    expect(/fill="(?:#4176E6|#679EFE)"/.test(favicon)).toBe(true);
    for (const old of ["#863bff", "#7e14ff", "#aa3bff", "#47bfff"]) {
      expect(favicon.toLowerCase()).not.toContain(old.toLowerCase());
    }
  });
  it("is a well-formed single svg root", () => {
    expect(favicon.trim().startsWith("<svg")).toBe(true);
    expect(favicon.trim().endsWith("</svg>")).toBe(true);
    expect((favicon.match(/<svg/g) ?? []).length).toBe(1);
  });
  it("purges the unreferenced brand-purple sprite from public/", () => {
    // icons.svg (5× #aa3bff fills) was dead weight — no tsx/css/html loads it.
    expect(existsSync(join(PUBLIC, "icons.svg"))).toBe(false);
    for (const name of readdirSync(PUBLIC)) {
      const text = readFileSync(join(PUBLIC, name), "utf8").toLowerCase();
      for (const old of ["#863bff", "#7e14ff", "#aa3bff", "#47bfff"]) {
        expect(text).not.toContain(old);
      }
    }
  });
});
