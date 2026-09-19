import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
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

  it("allows empty port and defaults to 4096", () => {
    const onConnect = vi.fn();
    render(<ConnectionBar onConnect={onConnect} />);
    fireEvent.change(screen.getByLabelText(/^host$/i), { target: { value: "127.0.0.1" } });
    fireEvent.click(screen.getByRole("button", { name: /connect/i }));
    expect(onConnect).toHaveBeenCalledTimes(1);
    expect(onConnect).toHaveBeenCalledWith("127.0.0.1", "4096", "");
    expect(screen.queryByText(/port required/i)).not.toBeInTheDocument();
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

describe("ConnectionBar stored prefill", () => {
  beforeEach(() => localStorage.clear());
  afterEach(() => localStorage.clear());

  it("prefills the last saved connection", () => {
    localStorage.setItem(
      "agentj.connection",
      JSON.stringify({ host: "192.168.3.31", port: "5000", token: "sek" }),
    );
    render(<ConnectionBar onConnect={() => {}} />);
    expect(screen.getByLabelText(/^host$/i)).toHaveValue("192.168.3.31");
    expect(screen.getByLabelText(/^port$/i)).toHaveValue("5000");
    expect(screen.getByLabelText(/^token$/i)).toHaveValue("sek");
  });
});