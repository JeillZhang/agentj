import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import ProjectEditDialog from "./ProjectEditDialog";
import { createProject, loadProjects, updateProject } from "../projectStore";

describe("ProjectEditDialog", () => {
  beforeEach(() => localStorage.clear());

  it("validates the work dir and saves the patch", () => {
    const project = createProject("Alpha");
    const onClose = vi.fn();
    render(<ProjectEditDialog project={project} onClose={onClose} />);
    fireEvent.change(screen.getByLabelText("项目名称"), { target: { value: "Alpha2" } });
    fireEvent.change(screen.getByLabelText("工作目录"), { target: { value: "relative/path" } });
    expect(screen.getByText("须为绝对路径")).toBeTruthy();
    expect(screen.getByRole("button", { name: "保存" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("工作目录"), { target: { value: "/repo/alpha" } });
    fireEvent.click(screen.getByRole("switch", { name: "默认项目" }));
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    const saved = loadProjects().find((p) => p.id === project.id);
    expect(saved?.name).toBe("Alpha2");
    expect(saved?.workDir).toBe("/repo/alpha");
    expect(saved?.isDefault).toBe(true);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("clears the previous default project on save", () => {
    const a = createProject("Alpha");
    const b = createProject("Beta");
    updateProject(b.id, { isDefault: true });
    render(<ProjectEditDialog project={a} onClose={() => {}} />);
    fireEvent.click(screen.getByRole("switch", { name: "默认项目" }));
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    const list = loadProjects();
    expect(list.find((p) => p.id === a.id)?.isDefault).toBe(true);
    expect(list.find((p) => p.id === b.id)?.isDefault).toBeFalsy();
  });
});
