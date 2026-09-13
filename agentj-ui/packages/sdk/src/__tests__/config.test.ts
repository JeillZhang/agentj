import { describe, expect, it } from "vitest";
import { PROTOCOL_VERSION, VERSION, resolveWsUrl } from "../config.js";
import { METHODS } from "../methods.js";

describe("config", () => {
  it("version bumped to 0.14.0", () => {
    expect(VERSION).toBe("0.14.0");
    expect(PROTOCOL_VERSION).toBe("0.12.1");
  });

  it("resolveWsUrl appends token query", () => {
    expect(resolveWsUrl({ port: 5000, token: "s3cret" })).toBe(
      "ws://127.0.0.1:5000/ws?token=s3cret",
    );
  });

  it("resolveWsUrl honors explicit env URL", () => {
    expect(resolveWsUrl({ envUrl: "ws://host:1/ws" })).toBe("ws://host:1/ws");
  });

  it("methods cover the 16-method registry", () => {
    expect(METHODS.kernelStatus).toBe("kernel/status");
    expect(Object.keys(METHODS)).toHaveLength(18);
  });
});
