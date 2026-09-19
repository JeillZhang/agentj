import { describe, expect, test } from "vitest";
import { formatTokenTotal, formatUsageSummary, formatUsageTitle } from "./tokenUsage";

describe("formatTokenTotal", () => {
  test("boundaries", () => {
    expect(formatTokenTotal(0)).toBe("0");
    expect(formatTokenTotal(999)).toBe("999");
    expect(formatTokenTotal(1000)).toBe("1k");
    expect(formatTokenTotal(12345)).toBe("12.3k");
    expect(formatTokenTotal(999949)).toBe("999.9k");
    expect(formatTokenTotal(999950)).toBe("1M");
    expect(formatTokenTotal(999999)).toBe("1M");
    expect(formatTokenTotal(1000000)).toBe("1M");
    expect(formatTokenTotal(1234567)).toBe("1.2M");
  });
});

describe("formatUsageSummary", () => {
  test("small numbers render verbatim across all three segments", () => {
    expect(formatUsageSummary({ prompt: 12, completion: 34 })).toBe("Σ 46 · 输入 12 · 输出 34");
  });

  test("kilo values trim the trailing `.0`", () => {
    expect(formatUsageSummary({ prompt: 2000, completion: 1000 })).toBe("Σ 3k · 输入 2k · 输出 1k");
  });

  test("renders the total, input and output each through formatTokenTotal", () => {
    expect(formatUsageSummary({ prompt: 1200, completion: 345 })).toBe("Σ 1.5k · 输入 1.2k · 输出 345");
  });

  test("rolls over to M at the 999.95k edge", () => {
    expect(formatUsageSummary({ prompt: 999950, completion: 0 })).toBe("Σ 1M · 输入 1M · 输出 0");
  });

  test("zero prompt with nonzero completion still renders all three segments", () => {
    expect(formatUsageSummary({ prompt: 0, completion: 1200 })).toBe("Σ 1.2k · 输入 0 · 输出 1.2k");
  });
});

describe("formatUsageTitle", () => {
  test("renders exact input, output and total values", () => {
    const usage = { prompt: 1200, completion: 345 };
    expect(formatUsageTitle(usage)).toBe(
      `本会话累计：输入 ${(1200).toLocaleString()} · 输出 ${(345).toLocaleString()} · 合计 ${(1200 + 345).toLocaleString()}`,
    );
  });

  test("renders a zero prompt with a nonzero completion", () => {
    const usage = { prompt: 0, completion: 1200 };
    expect(formatUsageTitle(usage)).toBe(
      `本会话累计：输入 ${(0).toLocaleString()} · 输出 ${(1200).toLocaleString()} · 合计 ${(0 + 1200).toLocaleString()}`,
    );
  });
});
