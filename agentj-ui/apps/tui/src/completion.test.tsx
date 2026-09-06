import { describe, expect, test } from "bun:test";
import { completePath } from "./completion.tsx";

describe("completePath pure", () => {
  test("completion matches workspace file prefix", () => {
    expect(completePath("@src/ma", ["src/main.rs", "src/lib.rs"])).toContain("src/main.rs");
  });
  test("completion returns empty on no match", () => {
    expect(completePath("@zzz", ["src/main.rs"])).toEqual([]);
  });
});
