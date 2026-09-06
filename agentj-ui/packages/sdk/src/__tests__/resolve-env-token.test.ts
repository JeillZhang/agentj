import { describe, expect, it } from "vitest";
import { resolveWsUrl } from "../config.js";

describe("resolveWsUrl envUrl token", () => {
  it("appends token to envUrl without query", () => {
    expect(resolveWsUrl({ envUrl: "ws://host:1/ws", token: "abc" })).toBe(
      "ws://host:1/ws?token=abc",
    );
  });

  it("appends token with & when envUrl has query", () => {
    expect(resolveWsUrl({ envUrl: "ws://host:1/ws?foo=bar", token: "abc" })).toBe(
      "ws://host:1/ws?foo=bar&token=abc",
    );
  });

  it("returns envUrl unchanged when no token", () => {
    expect(resolveWsUrl({ envUrl: "ws://host:1/ws" })).toBe("ws://host:1/ws");
  });

  it("encodes token", () => {
    expect(resolveWsUrl({ envUrl: "ws://host:1/ws", token: "a b&c" })).toBe(
      "ws://host:1/ws?token=" + encodeURIComponent("a b&c"),
    );
  });
});
