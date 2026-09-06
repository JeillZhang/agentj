import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import "@testing-library/jest-dom/vitest";
import { describe, expect, it, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import ConnectionBar from "./ConnectionBar.tsx";

const cssPath = () =>
  join(dirname(fileURLToPath(import.meta.url)), "components/connection.css");

describe("ConnectionBar validation (BUG-001)", () => {
  it("blocks onConnect when host is empty and shows inline error", () => {
    const onConnect = vi.fn();
    render(<ConnectionBar onConnect={onConnect} />);
    fireEvent.change(screen.getByLabelText(/^port$/i), { target: { value: "4096" } });
    fireEvent.click(screen.getByRole("button", { name: /connect/i }));
    expect(onConnect).not.toHaveBeenCalled();
    expect(screen.getByText(/host required/i)).toBeInTheDocument();
    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(screen.getByLabelText(/^host$/i)).toHaveAttribute("aria-describedby", "host-error");
  });

  it("blocks onConnect when port is empty and shows inline error", () => {
    const onConnect = vi.fn();
    render(<ConnectionBar onConnect={onConnect} />);
    fireEvent.change(screen.getByLabelText(/^host$/i), { target: { value: "127.0.0.1" } });
    fireEvent.click(screen.getByRole("button", { name: /connect/i }));
    expect(onConnect).not.toHaveBeenCalled();
    expect(screen.getByText(/port required/i)).toBeInTheDocument();
    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(screen.getByLabelText(/^port$/i)).toHaveAttribute("aria-describedby", "port-error");
  });

  it("calls onConnect with host/port/token when fields are valid", () => {
    const onConnect = vi.fn();
    render(<ConnectionBar onConnect={onConnect} />);
    fireEvent.change(screen.getByLabelText(/^host$/i), { target: { value: "10.0.0.1" } });
    fireEvent.change(screen.getByLabelText(/^port$/i), { target: { value: "7000" } });
    fireEvent.change(screen.getByLabelText(/token/i), { target: { value: "sek" } });
    fireEvent.click(screen.getByRole("button", { name: /connect/i }));
    expect(onConnect).toHaveBeenCalledTimes(1);
    expect(onConnect).toHaveBeenCalledWith("10.0.0.1", "7000", "sek");
  });
});

describe("ConnectionBar card + form polish (T3)", () => {
  it("connection card has labeled 40px inputs and no emoji", () => {
    render(<ConnectionBar onConnect={() => {}} />);
    expect(document.querySelector(".aj-connection-card")).not.toBeNull();
    expect(screen.getByLabelText(/host/i)).toBeInTheDocument();
    expect(screen.getByLabelText(/port/i)).toBeInTheDocument();
    const css = readFileSync(cssPath(), "utf8");
    expect(css).toContain("aj-connection-card");
    expect(css).toContain("40px");
    expect(screen.queryByText("✓")).toBeNull();
  });

  it("uses lucide icons with aria-hidden and 16px instead of emoji", () => {
    const { container } = render(<ConnectionBar onConnect={() => {}} />);
    const icons = container.querySelectorAll("svg[aria-hidden='true']");
    expect(icons.length).toBeGreaterThanOrEqual(3);
    expect(container.textContent).not.toMatch(/[✓🔌🔗🗝]/);
  });
});