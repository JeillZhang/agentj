import { describe, expect, it } from "vitest";
import { parseProgress, type ProgressFrame } from "../progress.ts";
import { METHODS } from "../methods.ts";

describe("parseProgress standard wire frames", () => {
  it("parses text deltas", () => {
    const f = parseProgress({ token: 7, value: { kind: "text", delta: "hello" } });
    expect(f).toEqual({ kind: "text", text: "hello" });
  });
  it("parses tool_call with the nested tool object flattened", () => {
    const f = parseProgress({
      token: 7,
      value: { kind: "tool_call", tool_call: { id: "c1", name: "bash", arguments: { cmd: "ls" } } },
    });
    expect(f).toEqual({ kind: "tool_call", id: "c1", name: "bash", arguments: { cmd: "ls" } });
  });
  it("parses tool_result", () => {
    const f = parseProgress({ token: 7, value: { kind: "tool_result", tool_call_id: "c1", content: "auth.rs\n" } });
    expect(f).toEqual({ kind: "tool_result", call_id: "c1", content: "auth.rs\n" });
  });
  it("parses usage settlement", () => {
    const f = parseProgress({ token: 7, value: { kind: "usage", usage: { prompt_tokens: 10, completion_tokens: 5 } } });
    expect(f).toEqual({ kind: "usage", usage: { prompt_tokens: 10, completion_tokens: 5 } });
  });
  it("parses permission settlement with optional session_id", () => {
    const f = parseProgress({
      token: null,
      value: { kind: "permission", request_id: "r1", tool: "bash", decision: "allow", session_id: "s-1" },
    });
    expect(f).toEqual({ kind: "permission", request_id: "r1", tool: "bash", decision: "allow", session_id: "s-1" });
  });
  it("tolerates a missing session_id on permission", () => {
    const f = parseProgress({ token: null, value: { kind: "permission", request_id: "r1", tool: "bash", decision: "deny" } });
    expect(f?.kind).toBe("permission");
  });
  it("returns an unknown frame for unknown kind, non-object params, and missing value (never null)", () => {
    const a = parseProgress({ token: 1, value: { kind: "telemetry", x: 1 } });
    expect(a).toEqual({ kind: "unknown", raw: { token: 1, value: { kind: "telemetry", x: 1 } } });
    expect(parseProgress("junk")).toMatchObject({ kind: "unknown" });
    expect(parseProgress({ token: 1 })).toMatchObject({ kind: "unknown" });
    expect(parseProgress(null)).toMatchObject({ kind: "unknown" });
    expect(parseProgress({ token: 1, value: { kind: "text" } })).toMatchObject({ kind: "unknown" });
  });
  it("parses flat legacy frames as text frames (Concretization 11 tolerance)", () => {
    expect(parseProgress({ session_id: "s1", text: "hello" })).toEqual({ kind: "text", text: "hello" });
    expect(parseProgress({ text: "hello" })).toEqual({ kind: "text", text: "hello" });
  });
});

describe("regenerated method constants", () => {
  it("exposes the two new methods", () => {
    expect(METHODS.sessionList).toBe("session/list");
    expect(METHODS.sessionHistory).toBe("session/history");
  });
});
