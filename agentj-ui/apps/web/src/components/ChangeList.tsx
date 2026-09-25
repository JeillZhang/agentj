import { useEffect, useRef, useState } from "react";
import type { FileChangeRow } from "../changes";
import { sortBySeq } from "../changes";
import { useRightSection } from "../rightSections";

export type ChangeListProps = {
  changes?: FileChangeRow[];
  rows?: FileChangeRow[];
  history?: FileChangeRow[];
  emptyText?: string;
  highlightedPath?: string | null;
  activeRunId?: string | null;
  onSelectRun?: (runId: string | null) => void;
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

function keyOf(row: FileChangeRow): string {
  return `${row.runId ?? "__legacy__"}\0${row.path}`;
}

function dedup(rows: FileChangeRow[]): FileChangeRow[] {
  const map = new Map<string, FileChangeRow>();
  for (const row of rows) map.set(keyOf(row), row);
  return [...map.values()];
}

function SourceBadge({ row }: { row: FileChangeRow }) {
  const effectiveSource = row.source ?? (row.diff ? "write" : undefined);
  if (effectiveSource === "bash") {
    return (
      <span className="src-badge src-bash" title="bash 变更无 diff，仅计数">
        bash
      </span>
    );
  }
  if (effectiveSource === "write") {
    return <span className="src-badge src-write">write</span>;
  }
  return null;
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
      <SourceBadge row={row} />
    </>
  );
}

function DiffPanel({ row }: { row: FileChangeRow }) {
  const trunc =
    row.diffTruncated && typeof row.totalBytes === "number" ? (
      <div className="trunc-hint">已截断（原始 {Math.ceil(row.totalBytes / 1024)} KiB）</div>
    ) : null;
  return (
    <div className="diff-wrap">
      <div className="diff-head">
        <strong>{row.path}</strong>
        <span>· unified diff</span>
        <span className="add">+{row.additions}</span>
        {row.deletions !== null && <span className="del">-{row.deletions}</span>}
      </div>
      {trunc}
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

const NULL_TOOL = "\0null";

function toolGroups(entries: FileChangeRow[]): { toolId: string | null; rows: FileChangeRow[] }[] {
  const order: (string | null)[] = [];
  const buckets = new Map<string, FileChangeRow[]>();
  for (const entry of entries) {
    const key = entry.toolCallId ?? NULL_TOOL;
    let bucket = buckets.get(key);
    if (!bucket) {
      bucket = [];
      buckets.set(key, bucket);
      order.push(entry.toolCallId ?? null);
    }
    bucket.push(entry);
  }
  return order.map((toolId) => ({ toolId, rows: buckets.get(toolId ?? NULL_TOOL) ?? [] }));
}

function HistoryDrawer({ row, allRows }: { row: FileChangeRow; allRows: FileChangeRow[] }) {
  const entries = allRows.filter((r) => (r.runId ?? null) === (row.runId ?? null) && r.path === row.path);
  if (entries.length <= 1) return null;
  const groups = toolGroups(sortBySeq(entries));
  const count = entries.length;
  return (
    <details className="history-drawer">
      <summary>历史记录 ({count})</summary>
      <div className="history-list">
        {groups.map((group, gi) => (
          <div className="hist-toolgroup" key={`${group.toolId ?? "no-id"}-${gi}`}>
            {group.toolId !== null && <div className="hist-tool">{group.toolId}</div>}
            {group.rows.map((entry, idx) => (
              <div className="history-entry" key={`${entry.seq ?? entry.eventSeq ?? idx}-${idx}`}>
                <span className="hist-seq">seq {entry.seq ?? entry.eventSeq ?? "-"}</span>
                <span className="hist-counts">
                  +{entry.additions}
                  {entry.deletions !== null ? ` -${entry.deletions}` : ""}
                </span>
                {entry.source && <span className="hist-src">{entry.source}</span>}
              </div>
            ))}
          </div>
        ))}
      </div>
    </details>
  );
}

export default function ChangeList({ changes, rows, history, emptyText, highlightedPath, activeRunId, onSelectRun: _onSelectRun }: ChangeListProps) {
  const [open, toggle] = useRightSection("changes");
  const [expandedKey, setExpandedKey] = useState<string | null>(null);
  const summaryRows = changes ?? rows ?? [];
  const displayRows = dedup(summaryRows);
  const allRows = history ?? changes ?? rows ?? [];
  const emptyLabel = emptyText ?? "暂无变更";
  const rowRefs = useRef<Map<string, HTMLElement>>(new Map());

  useEffect(() => {
    if (!highlightedPath) return;
    const keys = [
      `${activeRunId ?? "__legacy__"}\0${highlightedPath}`,
      `__legacy__\0${highlightedPath}`,
    ];
    for (const k of keys) {
      const el = rowRefs.current.get(k);
      if (el) {
        el.scrollIntoView?.({ behavior: "smooth", block: "nearest" });
        el.setAttribute("data-highlight", "true");
        const timer = setTimeout(() => el.removeAttribute("data-highlight"), 1500);
        return () => clearTimeout(timer);
      }
    }
    // fallback: any row with path suffix
    for (const [key, el] of rowRefs.current.entries()) {
      if (key.endsWith("\0" + highlightedPath)) {
        el.scrollIntoView?.({ behavior: "smooth", block: "nearest" });
        el.setAttribute("data-highlight", "true");
        const timer = setTimeout(() => el.removeAttribute("data-highlight"), 1500);
        return () => clearTimeout(timer);
      }
    }
  }, [highlightedPath, activeRunId, displayRows]);
  void _onSelectRun;

  return (
    <div className="right-section" data-section="changes" data-open={open ? "true" : undefined}>
      <button
        type="button"
        className="sec-head"
        aria-expanded={open}
        aria-controls="change-section-body"
        onClick={toggle}
      >
        <span>文件变更</span>
        <span className="count">{displayRows.length}</span>
        <span className="sec-chev" aria-hidden="true">
          {open ? "▾" : "▸"}
        </span>
      </button>
      <div id="change-section-body" className="sec-body">
        {open &&
          (displayRows.length === 0 ? (
            <p className="aj-empty">{emptyLabel}</p>
          ) : (
            <div className="file-changes">
              {displayRows.map((row) => {
                const compositeKey = keyOf(row);
                const effectiveSource = row.source ?? (row.diff ? "write" : undefined);
                const isBash = effectiveSource === "bash";
                const isExpandable = row.diff !== null && !isBash;
                const isOpen = expandedKey === compositeKey;
                if (!isExpandable) {
                  const title = isBash ? "bash 变更无 diff，仅计数" : undefined;
                  const isHighlighted = highlightedPath ? row.path === highlightedPath && (activeRunId == null || row.runId === activeRunId || row.runId == null) : false;
                  return (
                    <div
                      key={compositeKey}
                      ref={(el) => {
                        if (el) rowRefs.current.set(compositeKey, el);
                        else rowRefs.current.delete(compositeKey);
                      }}
                      data-path={row.path}
                      data-highlight={isHighlighted ? "true" : undefined}
                    >
                      <div className="fc" title={title}>
                        <RowMeta row={row} />
                      </div>
                      <HistoryDrawer row={row} allRows={allRows} />
                    </div>
                  );
                }
                return (
                  <div
                    key={compositeKey}
                    ref={(el) => {
                      if (el) rowRefs.current.set(compositeKey, el);
                      else rowRefs.current.delete(compositeKey);
                    }}
                    data-path={row.path}
                    data-highlight={highlightedPath === row.path && (activeRunId == null || row.runId === activeRunId || row.runId == null) ? "true" : undefined}
                  >
                    <button
                      type="button"
                      className="fc"
                      aria-expanded={isOpen}
                      onClick={() => setExpandedKey(isOpen ? null : compositeKey)}
                    >
                      <RowMeta row={row} />
                    </button>
                    {isOpen && <DiffPanel row={row} />}
                    <HistoryDrawer row={row} allRows={allRows} />
                  </div>
                );
              })}
            </div>
          ))}
      </div>
    </div>
  );
}
