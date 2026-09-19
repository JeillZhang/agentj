import { describe, expect, it } from "vitest";
import { PROTOCOL_VERSION, VERSION, resolveWsUrl } from "../config.js";
import { METHODS } from "../methods.js";

describe("config", () => {
  it("version bumped to 0.17.0", () => {
    expect(VERSION).toBe("0.17.0");
    expect(PROTOCOL_VERSION).toBe("0.13.0");
  });

  it("resolveWsUrl appends token query", () => {
    expect(resolveWsUrl({ port: 5000, token: "s3cret" })).toBe(
      "ws://127.0.0.1:5000/ws?token=s3cret",
    );
  });

  it("resolveWsUrl honors explicit env URL", () => {
    expect(resolveWsUrl({ envUrl: "ws://host:1/ws" })).toBe("ws://host:1/ws");
  });

  it("methods cover the 21-method registry", () => {
    expect(METHODS.kernelStatus).toBe("kernel/status");
    expect(Object.keys(METHODS)).toHaveLength(21);
    expect(METHODS.fileList).toBe("file/list");
    expect(METHODS.askQuestion).toBe("$/askQuestion");
    expect(METHODS.questionRespond).toBe("question/respond");
  });

  it("resolveWsUrl derives the host from the page location", () => {
    expect(resolveWsUrl({ location: { hostname: "192.168.3.31", protocol: "http:" } })).toBe(
      "ws://192.168.3.31:4096/ws",
    );
  });

  it("resolveWsUrl lets an explicit host win over the page location", () => {
    expect(
      resolveWsUrl({
        host: "10.0.0.5",
        port: 5000,
        location: { hostname: "192.168.3.31", protocol: "http:" },
      }),
    ).toBe("ws://10.0.0.5:5000/ws");
  });

  it("resolveWsUrl upgrades to wss on an https page", () => {
    expect(resolveWsUrl({ location: { hostname: "example.com", protocol: "https:" } })).toBe(
      "wss://example.com:4096/ws",
    );
  });

  it("resolveWsUrl keeps loopback for the Tauri web build host", () => {
    expect(resolveWsUrl({ location: { hostname: "tauri.localhost", protocol: "http:" } })).toBe(
      "ws://127.0.0.1:4096/ws",
    );
  });

  it("resolveWsUrl ignores non-http(s) page protocols", () => {
    expect(resolveWsUrl({ location: { hostname: "localhost", protocol: "tauri:" } })).toBe(
      "ws://127.0.0.1:4096/ws",
    );
  });

  it("resolveWsUrl keeps the Tauri desktop on plaintext loopback for https pages", () => {
    expect(resolveWsUrl({ location: { hostname: "tauri.localhost", protocol: "https:" } })).toBe(
      "ws://127.0.0.1:4096/ws",
    );
  });
});
