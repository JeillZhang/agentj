import { useState } from "react";
import { File } from "lucide-react";
import type { FileNode } from "../demoData";
import "./filetree.css";

export type FileTreeProps = {
  nodes: FileNode[];
};

type FileTreeStats = {
  files: number;
  changes: number;
};

/** Recursive file/git-change counters drive the footer text from the data alone. */
function collectStats(nodes: FileNode[]): FileTreeStats {
  const stats: FileTreeStats = { files: 0, changes: 0 };
  const walk = (list: FileNode[]): void => {
    for (const node of list) {
      if (node.kind === "dir") {
        if (node.children) walk(node.children);
        continue;
      }
      stats.files += 1;
      if (node.git) stats.changes += 1;
    }
  };
  walk(nodes);
  return stats;
}

function childGroupId(path: string): string {
  return `filetree-${path}-children`;
}

type NodeListProps = {
  nodes: FileNode[];
  depth: number;
  expanded: Record<string, boolean>;
  onToggle: (path: string) => void;
};

/** Renders one level; dirs recurse only while expanded so collapsed leaves stay out of the DOM. */
function NodeList({ nodes, depth, expanded, onToggle }: NodeListProps) {
  return (
    <>
      {nodes.map((node) => {
        const indent = depth > 0 ? ` indent-${depth}` : "";
        if (node.kind === "dir") {
          const open = expanded[node.path] === true;
          const groupId = childGroupId(node.path);
          return (
            <div className="trow-group" key={node.path}>
              <button
                type="button"
                className={`trow${indent} trow-dir`}
                aria-expanded={open}
                aria-controls={groupId}
                onClick={() => onToggle(node.path)}
              >
                <span className="chev" aria-hidden="true">{open ? "▾" : "▸"}</span>
                <span className="name">{node.name}</span>
              </button>
              <div id={groupId}>
                {open && node.children ? (
                  <NodeList nodes={node.children} depth={depth + 1} expanded={expanded} onToggle={onToggle} />
                ) : null}
              </div>
            </div>
          );
        }
        return (
          <div className={`trow${indent}`} key={node.path}>
            <span className="chev" aria-hidden="true" />
            <File className="file-icon" size={12} aria-hidden="true" />
            <span className="name">{node.name}</span>
            {node.git ? (
              <>
                <span className={`status-txt status-${node.git}`}>{node.git}</span>
                <span className={`dot dot-${node.git}`} aria-hidden="true" />
              </>
            ) : null}
          </div>
        );
      })}
    </>
  );
}

/**
 * Left-panel 文件 tab body: expandable tree + git markers + footer stats.
 * Owns its local expansion state; T15 only supplies `DEMO_FILE_TREE`.
 */
export default function FileTree({ nodes }: FileTreeProps) {
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const { files, changes } = collectStats(nodes);

  const toggle = (path: string) => {
    setExpanded((prev) => ({ ...prev, [path]: prev[path] !== true }));
  };

  return (
    <div className="filetree">
      {nodes.length === 0 ? (
        <p className="aj-empty">待协议支持</p>
      ) : (
        <div className="tree">
          <NodeList nodes={nodes} depth={0} expanded={expanded} onToggle={toggle} />
        </div>
      )}
      <div className="left-foot">
        <span>{files} 个文件 · {changes} 处变更</span>
        <span>展开全部</span>
      </div>
    </div>
  );
}
