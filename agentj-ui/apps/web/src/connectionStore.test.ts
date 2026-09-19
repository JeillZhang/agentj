import { beforeEach, describe, expect, it } from "vitest";
import { loadConnection, saveConnection } from "./connectionStore";

describe("connectionStore", () => {
  beforeEach(() => localStorage.clear());

  it("returns null when nothing is stored", () => {
    expect(loadConnection()).toBeNull();
  });

  it("round-trips manual connection settings", () => {
    saveConnection({ host: "192.168.3.31", port: "4096", token: "s3cret" });
    expect(loadConnection()).toEqual({ host: "192.168.3.31", port: "4096", token: "s3cret" });
  });

  it("returns null for corrupt or incomplete records", () => {
    localStorage.setItem("agentj.connection", "{oops");
    expect(loadConnection()).toBeNull();
    localStorage.setItem("agentj.connection", JSON.stringify({ port: "1", token: "" }));
    expect(loadConnection()).toBeNull();
    localStorage.setItem("agentj.connection", JSON.stringify({ host: "h", port: 4096, token: "" }));
    expect(loadConnection()).toBeNull();
  });
});
