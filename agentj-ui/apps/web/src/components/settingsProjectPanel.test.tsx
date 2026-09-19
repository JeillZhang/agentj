import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import SettingsProjectPanel from "./SettingsProjectPanel";
import {
  assignSession,
  createProject,
  loadCurrentProject,
  loadProjectDefaults,
  loadProjects,
  updateProject,
} from "../projectStore";

describe("SettingsProjectPanel", () => {
  beforeEach(() => localStorage.clear());

  it("lists projects and guards the Inbox delete", () => {
    const alpha = createProject("Alpha");
    updateProject(alpha.id, { workDir: "/repo/alpha", isDefault: true });
    render(<SettingsProjectPanel />);
    expect(screen.getByText("Alpha")).toBeTruthy();
    expect(screen.getByText("/repo/alpha")).toBeTruthy();
    expect(screen.getByRole("button", { name: "删除项目 Inbox" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "删除项目 Alpha" })).toBeEnabled();
  });

  it("persists the global default project", () => {
    const alpha = createProject("Alpha");
    const beta = createProject("Beta");
    updateProject(alpha.id, { isDefault: true });
    render(<SettingsProjectPanel />);
    fireEvent.change(screen.getByLabelText("默认项目"), { target: { value: beta.id } });
    expect(loadProjects().find((p) => p.id === beta.id)?.isDefault).toBe(true);
    expect(loadProjects().find((p) => p.id === alpha.id)?.isDefault).toBeFalsy();
    expect(loadCurrentProject()).toBe(beta.id); // D11 switch source
  });

  it("persists new-project work dir default", () => {
    render(<SettingsProjectPanel />);
    fireEvent.change(screen.getByLabelText("新项目工作目录"), { target: { value: "/repo/base" } });
    expect(loadProjectDefaults().workDir).toBe("/repo/base");
  });

  it("confirms delete with the reassigned session count", () => {
    const alpha = createProject("Alpha");
    assignSession("s1", alpha.id);
    assignSession("s2", alpha.id);
    render(<SettingsProjectPanel />);
    fireEvent.click(screen.getByRole("button", { name: "删除项目 Alpha" }));
    expect(screen.getByText(/2 个会话将归属回 Inbox/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "删除" }));
    expect(loadProjects().some((p) => p.id === alpha.id)).toBe(false);
  });

  it("returns focus to the edit trigger after closing the dialog", async () => {
    createProject("Alpha");
    render(<SettingsProjectPanel />);
    const edit = screen.getByRole("button", { name: "编辑项目 Alpha" });
    fireEvent.click(edit);
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => expect(document.activeElement).toBe(edit)); // rAF-deferred refocus
  });
});
