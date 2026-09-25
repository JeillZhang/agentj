import type { TodoView, TodoState } from "../todoState";
import { useRightSection } from "../rightSections";

export type TodoPanelProps = {
  items: TodoView[];
  byRun?: Map<string | null, { revision: number; items: TodoView[] }> | null;
  state?: TodoState | null;
  activeRunId?: string | null;
  onSelectRun?: (runId: string | null) => void;
};

type StatusMeta = {
  cb: string;
  pill: string;
  label: string;
  done: boolean;
};

const STATUS_META: Record<TodoView["status"], StatusMeta> = {
  completed: { cb: "done", pill: "done", label: "已完成", done: true },
  in_progress: { cb: "prog", pill: "prog", label: "进行中", done: false },
  pending: { cb: "pending", pill: "todo", label: "待办", done: false },
  cancelled: { cb: "pending", pill: "todo", label: "已取消", done: false },
};
/** Right-panel 任务 section: counter + read-only todo rows. Styles live in rightpanel.css (T10). */
function TodoRows({ items }: { items: TodoView[] }) {
  if (items.length === 0) return <p className="aj-empty">暂无任务</p>;
  return (
    <div className="todo-list">
      {items.map((item) => {
        const meta = STATUS_META[item.status] ?? STATUS_META.pending;
        return (
          <div className="aj-todo" key={item.id}>
            <span className={`cb ${meta.cb}`}>{meta.done ? "✓" : ""}</span>
            <span className="todo-label">{item.content}</span>
            <span className={`pill-st ${meta.pill}`}>{meta.label}</span>
          </div>
        );
      })}
    </div>
  );
}

export default function TodoPanel({ items, byRun, state, activeRunId, onSelectRun }: TodoPanelProps) {
  const [open, toggle] = useRightSection("todo");
  const hasGrouping = (() => {
    if (byRun != null) return true;
    const br = (state as unknown as { byRun?: unknown })?.byRun;
    return br instanceof Map;
  })();
  const effByRun: Map<string | null, { revision: number; items: TodoView[] }> | null = (() => {
    if (byRun instanceof Map) return byRun;
    const br = (state as unknown as { byRun?: Map<string | null, { revision: number; items: TodoView[] }> })?.byRun;
    return br instanceof Map ? br : null;
  })();
  if (hasGrouping && effByRun) {
    const entries = [...effByRun.entries()];
    // currentRun pinning: activeRunId first, then by recency (insertion order already ok, keep stable)
    const sorted = activeRunId
      ? [...entries].sort((a, b) => {
          const ka = a[0] ?? "__legacy__";
          const kb = b[0] ?? "__legacy__";
          const aIsCurrent = ka === activeRunId;
          const bIsCurrent = kb === activeRunId;
          if (aIsCurrent && !bIsCurrent) return -1;
          if (!aIsCurrent && bIsCurrent) return 1;
          return 0;
        })
      : entries;
    const totalCompleted = items.filter((it) => it.status === "completed").length;
    return (
      <div className="right-section" data-section="todo" data-open={open ? "true" : undefined}>
        <button
          type="button"
          className="sec-head"
          aria-expanded={open}
          aria-controls="todo-section-body"
          onClick={toggle}
        >
          <span>任务</span>
          <span className="count">{totalCompleted} / {items.length}</span>
          <span className="sec-chev" aria-hidden="true">{open ? "▾" : "▸"}</span>
        </button>
        <div id="todo-section-body" className="sec-body">
          {open &&
            (sorted.length === 0 ? (
              <p className="aj-empty">暂无任务</p>
            ) : (
              <div className="todo-groups">
                {sorted.map(([runId, group]) => {
                  const label =
                    runId === null ? "未分组（历史）" : runId === activeRunId ? "本次运行" : runId.slice(0, 8);
                  return (
                    <div className="todo-group" key={runId ?? "__legacy__"} data-run={runId ?? "__legacy__"}>
                      <button
                        type="button"
                        className="todo-group__head"
                        onClick={() => onSelectRun?.(runId)}
                      >
                        {label}
                      </button>
                      <TodoRows items={group.items} />
                    </div>
                  );
                })}
              </div>
            ))}
        </div>
      </div>
    );
  }
  const completed = items.filter((item) => item.status === "completed").length;
  return (
    <div className="right-section" data-section="todo" data-open={open ? "true" : undefined}>
      <button
        type="button"
        className="sec-head"
        aria-expanded={open}
        aria-controls="todo-section-body"
        onClick={toggle}
      >
        <span>任务</span>
        <span className="count">{completed} / {items.length}</span>
        <span className="sec-chev" aria-hidden="true">{open ? "▾" : "▸"}</span>
      </button>
      <div id="todo-section-body" className="sec-body">
        {open && (items.length === 0 ? (
          <p className="aj-empty">暂无任务</p>
        ) : (
          <TodoRows items={items} />
        ))}
      </div>
    </div>
  );
}
