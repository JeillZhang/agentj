import type { TodoView } from "../todoState";
import { useRightSection } from "../rightSections";

export type TodoPanelProps = {
  items: TodoView[];
};

type StatusMeta = {
  cb: string;
  pill: string;
  label: string;
  done: boolean;
};

/** Protocol may add statuses; cancelled falls back to pending styling + 已取消. */
const STATUS_META: Record<TodoView["status"], StatusMeta> = {
  completed: { cb: "done", pill: "done", label: "已完成", done: true },
  in_progress: { cb: "prog", pill: "prog", label: "进行中", done: false },
  pending: { cb: "pending", pill: "todo", label: "待办", done: false },
  cancelled: { cb: "pending", pill: "todo", label: "已取消", done: false },
};

/** Right-panel 任务 section: counter + read-only todo rows. Styles live in rightpanel.css (T10). */
export default function TodoPanel({ items }: TodoPanelProps) {
  const [open, toggle] = useRightSection("todo");
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
          <p className="aj-empty">待协议支持</p>
        ) : (
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
        ))}
      </div>
    </div>
  );
}
