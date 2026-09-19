import { useCallback, useEffect, useRef, useState } from "react";
import { Pencil, Trash2 } from "lucide-react";
import type { Project } from "../projectStore";
import {
  deleteProject,
  getDefaultProject,
  loadProjectDefaults,
  loadProjects,
  saveProjectDefaults,
  sessionCountForProject,
  setCurrentProject,
  updateProject,
} from "../projectStore";
import ModalDialog from "./ModalDialog";
import ProjectEditDialog from "./ProjectEditDialog";
import "./settingsDialog.css";
import "./settingsProjectPanel.css";

/**
 * 项目 settings panel — global defaults (default project + new-project work dir)
 * plus a project list with inline edit and guarded delete. Re-reads persisted
 * state on `agentj:projects-updated` so sibling surfaces stay in sync.
 */
export default function SettingsProjectPanel() {
  const [projects, setProjects] = useState<Project[]>(loadProjects);
  const [defaults, setDefaults] = useState(loadProjectDefaults);
  const [editing, setEditing] = useState<Project | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<Project | null>(null);
  const editBtnRefs = useRef<Map<string, HTMLButtonElement>>(new Map());

  const refresh = useCallback(() => {
    setProjects(loadProjects());
    setDefaults(loadProjectDefaults());
  }, []);

  useEffect(() => {
    window.addEventListener("agentj:projects-updated", refresh);
    return () => window.removeEventListener("agentj:projects-updated", refresh);
  }, [refresh]);

  const closeEditing = useCallback(() => {
    const id = editing?.id;
    setEditing(null);
    if (!id) return;
    // rAF: focus once the dialog has unmounted and focus has returned to body.
    requestAnimationFrame(() => editBtnRefs.current.get(id)?.focus());
  }, [editing]);

  const changeDefault = (id: string) => {
    updateProject(id, { isDefault: true });
    setCurrentProject(id);
    refresh();
  };

  const changeWorkDir = (value: string) => {
    setDefaults(saveProjectDefaults({ workDir: value.trim() || null }));
  };

  const removeProject = (project: Project) => {
    deleteProject(project.id);
    setConfirmDelete(null);
    refresh();
  };

  const defaultProjectId = getDefaultProject().id;
  const deleteCount = confirmDelete ? sessionCountForProject(confirmDelete.id) : 0;

  return (
    <div className="aj-settings-group">
      <div className="aj-field">
        <label className="aj-label" htmlFor="aj-settings-default-project">
          默认项目
        </label>
        <select
          id="aj-settings-default-project"
          aria-label="默认项目"
          className="aj-input"
          value={defaultProjectId}
          onChange={(e) => changeDefault(e.target.value)}
        >
          {projects.map((project) => (
            <option key={project.id} value={project.id}>
              {project.isDefault ? `${project.name}（默认）` : project.name}
            </option>
          ))}
        </select>
      </div>

      <div className="aj-field">
        <label className="aj-label" htmlFor="aj-settings-new-workdir">
          新项目工作目录
        </label>
        <input
          id="aj-settings-new-workdir"
          aria-label="新项目工作目录"
          className="aj-input"
          value={defaults.workDir ?? ""}
          placeholder="/path/to/project"
          onChange={(e) => changeWorkDir(e.target.value)}
        />
        <span className="aj-hint">新建项目时预填的工作目录</span>
      </div>

      <ul className="aj-settings-project-list">
        {projects.map((project) => (
          <li key={project.id} className="aj-settings-project">
            <div className="aj-settings-project__info">
              <span className="aj-settings-project__title">
                <span className="aj-settings-project__name">{project.name}</span>
                {project.isDefault ? <span className="aj-settings-project__badge">默认</span> : null}
              </span>
              <span className="aj-settings-project__path">{project.workDir || "未设置"}</span>
            </div>
            <div className="aj-settings-project__actions">
              <button
                type="button"
                className="aj-icon-btn"
                aria-label={`编辑项目 ${project.name}`}
                ref={(el) => {
                  if (el) editBtnRefs.current.set(project.id, el);
                  else editBtnRefs.current.delete(project.id);
                }}
                onClick={() => setEditing(project)}
              >
                <Pencil size={15} aria-hidden="true" />
              </button>
              <button
                type="button"
                className="aj-icon-btn"
                aria-label={`删除项目 ${project.name}`}
                disabled={project.id === "default"}
                onClick={() => setConfirmDelete(project)}
              >
                <Trash2 size={15} aria-hidden="true" />
              </button>
            </div>
          </li>
        ))}
      </ul>

      {editing ? (
        <ProjectEditDialog
          project={editing}
          onClose={closeEditing}
          onSaved={() => {
            setProjects(loadProjects());
            closeEditing();
          }}
        />
      ) : null}

      {confirmDelete ? (
        <ModalDialog title="删除项目" onClose={() => setConfirmDelete(null)}>
          <p className="aj-dialog__text">
            {`删除项目「${confirmDelete.name}」？该项目下 ${deleteCount} 个会话将归属回 Inbox。此操作不可恢复。`}
          </p>
          <div className="aj-dialog__actions">
            <button type="button" className="aj-button--ghost" onClick={() => setConfirmDelete(null)}>
              取消
            </button>
            <button
              type="button"
              className="aj-button aj-button--danger"
              onClick={() => removeProject(confirmDelete)}
            >
              删除
            </button>
          </div>
        </ModalDialog>
      ) : null}
    </div>
  );
}
