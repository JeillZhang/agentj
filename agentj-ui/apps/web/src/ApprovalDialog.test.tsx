import "@testing-library/jest-dom/vitest";
import { describe, expect, it, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import ApprovalDialog, { fieldRows } from "./ApprovalDialog.tsx";

const req = { request_id: "r1", tool: "bash", args: { cmd: "ls", timeout: 5 }, session_id: "s-91", reason: "user asked" };

describe("fieldRows", () => {
  it("maps known fields to friendly labels", () => {
    const rows = fieldRows("bash", { cmd: "ls", timeout: 5 });
    expect(rows).toContainEqual({ label: "命令", value: "ls" });
    expect(rows).toContainEqual({ label: "超时", value: 5 });
  });
  it("maps write file/content", () => {
    const rows = fieldRows("write", { path: "src/auth.rs", content: "+x" });
    expect(rows.map((r) => r.label)).toEqual(["文件", "内容"]);
  });
  it("falls back to raw json for unknown fields", () => {
    const rows = fieldRows("frobnicate", { x: 1 });
    expect(rows).toContainEqual({ label: "args", value: { x: 1 } });
  });
});

describe("ApprovalDialog", () => {
  it("shows mapped rows and allow/deny", () => {
    const onRespond = vi.fn();
    render(<ApprovalDialog request={req} onRespond={onRespond} onClose={() => {}} />);
    expect(screen.getByText("命令")).toBeInTheDocument();
    expect(screen.getByText("ls")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^allow$/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^deny$/i })).toBeInTheDocument();
  });
  it("collapses long content and expands on click", () => {
    const long = "x".repeat(600);
    const onRespond = vi.fn();
    render(
      <ApprovalDialog
        request={{ ...req, tool: "write", args: { path: "a.rs", content: long } }}
        onRespond={onRespond}
        onClose={() => {}}
      />,
    );
    expect(screen.queryByText(long)).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /expand/i }));
    expect(screen.getByText(long)).toBeInTheDocument();
  });
  it("plain Allow carries the checkbox, Allow All always allows", () => {
    const onRespond = vi.fn();
    render(<ApprovalDialog request={req} onRespond={onRespond} onClose={() => {}} />);
    const cb = screen.getByRole("checkbox");
    fireEvent.click(cb);
    fireEvent.click(screen.getByRole("button", { name: /^allow$/i }));
    expect(onRespond).toHaveBeenCalledWith("allow_always");
    fireEvent.click(screen.getByRole("button", { name: /allow all/i }));
    expect(onRespond).toHaveBeenLastCalledWith("allow_always");
  });
  it("Deny always sends decision deny", () => {
    const onRespond = vi.fn();
    render(<ApprovalDialog request={req} onRespond={onRespond} onClose={() => {}} />);
    const cb = screen.getByRole("checkbox");
    fireEvent.click(cb);
    fireEvent.click(screen.getByRole("button", { name: /^deny$/i }));
    expect(onRespond).toHaveBeenCalledWith("deny");
  });
  it("keeps the literal session scope line", () => {
    render(<ApprovalDialog request={req} onRespond={() => {}} onClose={() => {}} />);
    expect(screen.getByText(/session: s-91/)).toBeInTheDocument();
  });
  it("renders error text when provided", () => {
    render(<ApprovalDialog request={req} error="expired" onRespond={() => {}} onClose={() => {}} />);
    expect(screen.getByText("expired")).toBeInTheDocument();
  });
});
