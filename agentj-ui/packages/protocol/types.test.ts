import type { InitializeParams, SessionRunParams, SessionRunResult, UsageInfo } from "./types.js";
import { expectTypeOf, test } from "vitest";

test("protocol types are importable", () => {
  expectTypeOf<InitializeParams>().toHaveProperty("protocol_version");
  expectTypeOf<SessionRunParams>().toHaveProperty("session_id");
  expectTypeOf<SessionRunResult>().toHaveProperty("content");
  expectTypeOf<UsageInfo>().toHaveProperty("prompt_tokens");
});
