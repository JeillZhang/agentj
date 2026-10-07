import type { InitializeParams, SessionRunParams, SessionRunResult, UsageInfo } from "./types.js";
import { expectTypeOf, test } from "vitest";

test("protocol types are importable", () => {
  expectTypeOf<InitializeParams>().toHaveProperty("protocol_version");
  expectTypeOf<SessionRunParams>().toHaveProperty("session_id");
  expectTypeOf<SessionRunResult>().toHaveProperty("content");
  expectTypeOf<UsageInfo>().toHaveProperty("prompt_tokens");
});

test("SessionRunParams carries the runtime provider/model overrides", () => {
  const p: SessionRunParams = {
    session_id: "s1",
    content: "hi",
    provider: "opencode-go",
    model: "opencode-go/kimi-k3",
  };
  const provider: string | null | undefined = p.provider;
  const model: string | null | undefined = p.model;
  void [provider, model];
});
