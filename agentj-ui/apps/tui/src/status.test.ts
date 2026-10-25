import { describe, expect, it } from "bun:test";
import { formatSubSuffix, getStatusLine } from "./status";

describe("formatSubSuffix", () => {
  it("formats the worst window and flags stale data", () => {
    expect(formatSubSuffix({ windows: [{ percent: 12 }, { percent: 40 }], fetchedAt: 0, stale: false }))
      .toEqual({ text: "· sub 12%/40%", tone: "default" });
    expect(formatSubSuffix({ windows: [{ percent: 92 }], fetchedAt: 0, stale: false }).tone).toBe("error");
    expect(formatSubSuffix({ windows: [{ percent: 12 }], fetchedAt: 0, stale: true }).text).toContain("~");
  });

  it("grades the worst window into warning and error tones", () => {
    expect(formatSubSuffix({ windows: [{ percent: 12 }, { percent: 70 }] }).tone).toBe("warning");
    expect(formatSubSuffix({ windows: [{ percent: 89 }] }).tone).toBe("warning");
    expect(formatSubSuffix({ windows: [{ percent: 90 }] }).tone).toBe("error");
    expect(formatSubSuffix({ windows: [{ percent: 5 }] }).tone).toBe("default");
  });

  it("omits the suffix when there is no credential or no window", () => {
    expect(formatSubSuffix(null)).toEqual({ text: "", tone: "default" });
    expect(formatSubSuffix({ windows: [] })).toEqual({ text: "", tone: "default" });
    expect(formatSubSuffix({ windows: [{ percent: null }] })).toEqual({ text: "", tone: "default" });
  });
});

describe("getStatusLine", () => {
  it("keeps the connection wording and appends the subscription suffix", () => {
    expect(getStatusLine(null, "open")).toBe("connected");
    expect(getStatusLine(null, "closed")).toBe("kernel unreachable");
    expect(getStatusLine(null, "connecting")).toBe("connecting...");
    expect(getStatusLine("bad key", "open")).toBe("auth error: bad key");
    expect(getStatusLine(null, "open", { windows: [{ percent: 12 }, { percent: 40 }] }))
      .toBe("connected · sub 12%/40%");
    expect(getStatusLine(null, "open", { windows: [] })).toBe("connected");
  });
});
