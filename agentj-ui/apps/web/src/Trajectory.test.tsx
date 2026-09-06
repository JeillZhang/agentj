import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import "@testing-library/jest-dom/vitest";
import { describe, expect, it } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { Trajectory, type RawEvent } from "./Trajectory.tsx";

const CORPUS: RawEvent[] = [
  { session_id: "s1", seq: 1, ts: 100, value: { kind: "user", content: "hi" } },
  { session_id: "s1", seq: 2, ts: 101, value: { kind: "thinking", content: "hello world" } },
];

describe("Trajectory playback button (BUG-002)", () => {
  it("renders the play button with a lucide icon instead of unicode ▶", () => {
    const { container } = render(<Trajectory events={CORPUS} />);
    const button = screen.getByRole("button", { name: /^play$/i });
    // lucide icon renders as an svg with aria-hidden
    expect(button.querySelector("svg[aria-hidden='true']")).not.toBeNull();
    // no unicode play/pause text symbols anywhere in the button
    expect(button.textContent).not.toContain("▶");
    expect(button.textContent).not.toContain("⏸");
    // decorative icon is not exposed to the accessibility tree
    expect(container.querySelector("svg[aria-hidden='true']")).not.toBeNull();
  });

  it("shows the lucide pause icon while playing (no unicode ⏸)", () => {
    const { container } = render(<Trajectory events={CORPUS} />);
    fireEvent.click(screen.getByRole("button", { name: /^play$/i }));
    const button = screen.getByRole("button", { name: /pause/i });
    expect(button.querySelector("svg[aria-hidden='true']")).not.toBeNull();
    expect(button.textContent).not.toContain("⏸");
    expect(button.textContent).not.toContain("▶");
    expect(container.querySelector("svg[aria-hidden='true']")).not.toBeNull();
  });
});

describe("Trajectory timeline polish (T7)", () => {
  it("trajectory css has vertical line and shadowSoft", () => {
    const css = readFileSync(
      join(dirname(fileURLToPath(import.meta.url)), "components/trajectory.css"),
      "utf8",
    );
    expect(css).toContain("border-left");
    expect(css).toContain("shadowSoft");
  });

  it("trajectory handles empty and chips wrap", () => {
    const src = readFileSync(
      join(dirname(fileURLToPath(import.meta.url)), "Trajectory.tsx"),
      "utf8",
    );
    expect(src).toContain("aj-traj");
  });
});
