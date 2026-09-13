import { useState } from "react";
import type { FileChangeRow } from "../changes";

export type ChangeListProps = {
  changes: FileChangeRow[];
};

const OP_BADGE: Record<FileChangeRow["op"], { label: string; className: string }> = {
  add: { label: "新增", className: "badge-new" },
  modify: { label: "修改", className: "badge-mod" },
  delete: { label: "删除", className: "badge-mod" },
};

/** TypeScript files collapse to a TS badge; otherwise the uppercased extension (TS fallback). */
function fileBadge(path: string): string {
  const dot = path.lastIndexOf(".");
  if (dot === -1) return "TS";
  const ext = path.slice(dot + 1).toUpperCase();
  return ext === "TS" || ext === "TSX" ? "TS" : ext || "TS";
}

function lineClass(line: string): "add" | "del" | "ctx" {
  if (line.startsWith("+")) return "add";
  if (line.startsWith("-")) return "del";
  return "ctx";
}

function RowMeta({ row }: { row: FileChangeRow }) {
  const badge = OP_BADGE[row.op];
  return (
    <>
      <span className="fc-icon">{fileBadge(row.path)}</span>
      <span className="fc-path">{row.path}</span>
      <span className="fc-counts">
        <span className="add">+{row.additions}</span>
        {row.deletions !== null && <span className="del">-{row.deletions}</span>}
      </span>
      <span className={`badge ${badge.className}`}>{badge.label}</span>
    </>
  );
}

function DiffPanel({ row }: { row: FileChangeRow }) {
  return (
    <div className="diff-wrap">
      <div className="diff-head">
        <strong>{row.path}</strong>
        <span>· unified diff</span>
        <span className="add">+{row.additions}</span>
        {row.deletions !== null && <span className="del">-{row.deletions}</span>}
      </div>
      <div className="diff">
        {row.diff?.split("\n").map((line, index) => (
          <div key={index} className={`diff-line ${lineClass(line)}`}>
            {line}
          </div>
        ))}
      </div>
    </div>
  );
}

export default function ChangeList({ changes }: ChangeListProps) {
  const [open, setOpen] = useState(true);
  const [expandedPath, setExpandedPath] = useState<string | null>(null);

  return (
    <div className="right-section">
      <button
        type="button"
        className="sec-head"
        aria-expanded={open}
        aria-controls="change-section-body"
        onClick={() => setOpen((prev) => !prev)}
      >
        <span>文件变更</span>
        <span className="count">{changes.length}</span>
        <span className="sec-chev" aria-hidden="true">{open ? "▾" : "▸"}</span>
      </button>
      <div id="change-section-body" className="sec-body">
        {changes.length === 0 ? (
          <p className="aj-empty">待协议支持</p>
        ) : open && (
          <div className="file-changes">
            {changes.map((row) => {
              if (row.diff === null) {
                return (
                  <div className="fc" key={row.path}>
                    <RowMeta row={row} />
                  </div>
                );
              }
              const isOpen = expandedPath === row.path;
              return (
                <div key={row.path}>
                  <button
                    type="button"
                    className="fc"
                    aria-expanded={isOpen}
                    onClick={() => setExpandedPath(isOpen ? null : row.path)}
                  >
                    <RowMeta row={row} />
                  </button>
                  {isOpen && <DiffPanel row={row} />}
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
