import type {
  AuthDevicePollResult,
  CredentialInfo,
  InitializeParams,
  ModelEntry,
  ModelListResult,
  ProviderUsageResult,
  SessionRunParams,
  SessionRunResult,
  UsageInfo,
} from "./types.js";
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

test("subscription methods expose the new result shapes", () => {
  const catalog: ModelListResult = {
    provider: "opencode-go",
    default_model: "opencode-go/kimi-k3",
    source: "account",
    fetched_at: 1,
    models: [{ id: "opencode-go/kimi-k3", verified: true, format: "anthropic" } satisfies ModelEntry],
  };
  const usage: ProviderUsageResult = {
    provider: "opencode-go",
    fetched_at: 1,
    stale: false,
    warn_percent: 80,
    windows: [{ kind: "rolling", status: "ok", percent: 4 }],
  };
  const poll: AuthDevicePollResult = { state: "pending" };
  const cred: CredentialInfo = { kind: "api_key", source: "file", expiring_soon: false };
  void [catalog, usage, poll, cred];
});
