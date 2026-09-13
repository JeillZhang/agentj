import type { TodoItem } from "../demoData";

export type TodoPanelProps = {
  items: TodoItem[];
};

type StatusMeta = {
  cb: string;
  pill: string;
  label: string;
  done: boolean;
};

/** Protocol may add statuses; cancelled falls back to pending styling + 已取消. */
const STATUS_META: Record<TodoItem["status"], StatusMeta> = {
  completed: { cb: "done", pill: "done", label: "已完成", done: true },
  in_progress: { cb: "prog", pill: "prog", label: "进行中", done: false },
  pending: { cb: "pending", pill: "todo", label: "待办", done: false },
  cancelled: { cb: "pending", pill: "todo", label: "已取消", done: false },
};

/** Right-panel 任务 section: counter + read-only todo rows. Styles live in rightpanel.css (T10). */
export default function TodoPanel({ items }: TodoPanelProps) {
  if (items.length === 0) {
    return <p className="aj-empty">待协议支持</p>;
  }
  const completed = items.filter((item) => item.status === "completed").length;
  return (
    <>
      <div className="sec-head">
        <span>任务</span>
        <span className="count">{completed} / {items.length}</span>
      </div>
      <div className="todo-list">
        {items.map((item) => {
          const meta = STATUS_META[item.status];
          return (
            <div className="aj-todo" key={item.id}>
              <span className={`cb ${meta.cb}`}>{meta.done ? "✓" : ""}</span>
              <span className="todo-label">{item.content}</span>
              <span className={`pill-st ${meta.pill}`}>{meta.label}</span>
            </div>
          );
        })}
      </div>
    </>
  );
}
