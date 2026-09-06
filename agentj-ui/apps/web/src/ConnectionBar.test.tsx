import "@testing-library/jest-dom/vitest";
import { describe, expect, it, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import ConnectionBar from "./ConnectionBar.tsx";

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