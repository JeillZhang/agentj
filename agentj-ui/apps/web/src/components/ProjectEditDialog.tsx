import { useState } from "react";
import type { Project } from "../projectStore";
import { updateProject } from "../projectStore";
import { AGENT_LIST, MODEL_LIST } from "./ComposerOptions";
import ModalDialog from "./ModalDialog";
import "./settingsDialog.css";

export type ProjectEditDialogProps = {
  project: Project;
  onClose: () => void;
  onSaved?: (p: Project) => void;
};

/**
 * Modal form for editing an existing project's name, work dir, default flag
 * and per-project model/agent overrides. Seeded from `project`; the save button
 * is disabled until the name is non-empty and the work dir (when set) is
 * absolute. Delegates persistence and default-project exclusivity to
 * `updateProject`.
 */
export default function ProjectEditDialog({ project, onClose, onSaved }: ProjectEditDialogProps) {
  const [name, setName] = useState(project.name);
  const [workDir, setWorkDir] = useState(project.workDir ?? "");
  const [isDefault, setIsDefault] = useState(project.isDefault ?? false);
  const [model, setModel] = useState(project.defaultModel ?? "");
  const [agent, setAgent] = useState(project.defaultAgent ?? "");

  const relativeWorkDir = workDir !== "" && !workDir.startsWith("/");
  const invalid = !name.trim() || relativeWorkDir;

  const save = () => {
    if (invalid) return;
    const updated = updateProject(project.id, {
      name,
      workDir: workDir.trim() || null,
      isDefault,
      defaultModel: model || null,
      defaultAgent: agent || null,
    });
    if (updated) onSaved?.(updated);
    onClose();
  };

  return (
    <ModalDialog title="编辑项目" onClose={onClose}>
      <div className="aj-field">
        <label htmlFor="aj-project-name" className="aj-label">
          项目名称
        </label>
        <input
          id="aj-project-name"
          className="aj-input"
          autoFocus
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="项目名称"
        />
      </div>

      <div className="aj-field">
        <label htmlFor="aj-project-workdir" className="aj-label">
          工作目录
        </label>
        <input
          id="aj-project-workdir"
          className="aj-input"
          value={workDir}
          onChange={(e) => setWorkDir(e.target.value)}
          placeholder="/path/to/project"
          aria-invalid={relativeWorkDir}
        />
        {relativeWorkDir ? (
          <span className="aj-hint aj-hint--error">须为绝对路径</span>
        ) : (
          <span className="aj-hint">留空则使用当前工作目录</span>
        )}
      </div>

      <div className="aj-settings-toggle">
        <span>默认项目</span>
        <button
          type="button"
          role="switch"
          aria-label="默认项目"
          aria-checked={isDefault}
          className="aj-settings-switch"
          onClick={() => setIsDefault((current) => !current)}
        >
          {isDefault ? "已开启" : "已关闭"}
        </button>
      </div>

      <div className="aj-field">
        <label htmlFor="aj-project-model" className="aj-label">
          默认模型
        </label>
        <select
          id="aj-project-model"
          className="aj-input"
          value={model}
          onChange={(e) => setModel(e.target.value)}
        >
          <option value="">未设置</option>
          {MODEL_LIST.map((entry) => (
            <option key={entry.name} value={entry.name}>
              {entry.name}
            </option>
          ))}
        </select>
      </div>

      <div className="aj-field">
        <label htmlFor="aj-project-agent" className="aj-label">
          默认 Agent
        </label>
        <select
          id="aj-project-agent"
          className="aj-input"
          value={agent}
          onChange={(e) => setAgent(e.target.value)}
        >
          <option value="">未设置</option>
          {AGENT_LIST.map((entry) => (
            <option key={entry.id} value={entry.id}>
              {entry.label}
            </option>
          ))}
        </select>
      </div>

      <div className="aj-dialog__actions">
        <button type="button" className="aj-button--ghost" onClick={onClose}>
          取消
        </button>
        <button type="button" className="aj-button" disabled={invalid} onClick={save}>
          保存
        </button>
      </div>
    </ModalDialog>
  );
}
