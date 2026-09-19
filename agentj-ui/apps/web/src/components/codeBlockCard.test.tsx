// agentj-ui/apps/web/src/components/codeBlockCard.test.tsx
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import CodeBlockCard from "./CodeBlockCard";
import { copyText } from "../copyText";

vi.mock("../copyText", () => ({ copyText: vi.fn() }));

const code = <code className="language-rust">{"fn main() {}"}</code>;

describe("CodeBlockCard", () => {
  beforeEach(() => vi.mocked(copyText).mockReset());

  it("shows the language and toggles collapse", () => {
    render(<CodeBlockCard>{code}</CodeBlockCard>);
    expect(screen.getByText("rust")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "折叠代码" }));
    expect((document.querySelector(".aj-code__body") as HTMLPreElement).hidden).toBe(true);
    expect(screen.getByRole("button", { name: "展开代码" })).toBeTruthy();
  });

  it("copies and shows 已复制", async () => {
    vi.mocked(copyText).mockResolvedValue(true);
    render(<CodeBlockCard>{code}</CodeBlockCard>);
    fireEvent.click(screen.getByRole("button", { name: "复制代码" }));
    expect(await screen.findByText("已复制")).toBeTruthy();
  });

  it("shows 复制失败 when copyText resolves false", async () => {
    vi.mocked(copyText).mockResolvedValue(false);
    render(<CodeBlockCard>{code}</CodeBlockCard>);
    fireEvent.click(screen.getByRole("button", { name: "复制代码" }));
    expect(await screen.findByText("复制失败")).toBeTruthy();
  });

  it("keeps collapsed state across streaming re-renders", () => {
    const { rerender } = render(<CodeBlockCard>{code}</CodeBlockCard>);
    fireEvent.click(screen.getByRole("button", { name: "折叠代码" }));
    rerender(<CodeBlockCard>{<code className="language-rust">{"fn main() {} // more"}</code>}</CodeBlockCard>);
    expect((document.querySelector(".aj-code__body") as HTMLPreElement).hidden).toBe(true);
  });

  it("renders plain/unknown-language blocks with the fallback label", () => {
    render(<CodeBlockCard>{<code>{"plain text"}</code>}</CodeBlockCard>);
    expect(screen.getByText("code")).toBeTruthy();
    expect(screen.getByRole("button", { name: "复制代码" })).toBeTruthy();
  });
});
