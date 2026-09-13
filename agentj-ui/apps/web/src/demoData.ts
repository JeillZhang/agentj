// Demo-mode fixtures and gate for the three-column workbench.
// Copy is taken verbatim from docs/design/three-column/mockup-*.html so demo mode is
// pixel-comparable to docs/design/three-column/preview-01..08.png.
// Protocol gap: methods.json has no TODO/files/diff/Q&A events yet
// (see docs/design/three-column/protocol-extension-prompt.md), so these are static
// placeholders shown only while demo mode is on.

import type { FileChangeRow } from "./changes";

const DEMO_KEY = "agentj.demo";
const DEMO_EVENT = "agentj:demo-updated";

/** Todo row shape shared with TodoPanel (T8). */
export type TodoItem = {
  id: string;
  content: string;
  status: "pending" | "in_progress" | "completed" | "cancelled";
};

/** File-tree node shape — single source for FileTree (T14); set `git` explicitly on files. */
export type FileNode = {
  name: string;
  path: string;
  kind: "file" | "dir";
  git?: "M" | "A" | "U" | "D" | null;
  children?: FileNode[];
};

/** Ask-card option shape (T20/T21). */
export type DemoQuestionOption = {
  id: string;
  label: string;
  hint?: string;
};

/** Ask-card question shape (T20/T21). */
export type DemoQuestion = {
  id: string;
  question: string;
  options: DemoQuestionOption[];
};

/** Answered Q&A record shown collapsed in the feed (T21). */
export type DemoQaRecord = {
  id: string;
  question: string;
  answer: string;
  note: string;
};

/** `?demo=1` wins over the `agentj.demo` flag; guarded so non-DOM imports do not throw. */
export function isDemoEnabled(): boolean {
  if (typeof window === "undefined") return false;
  if (new URLSearchParams(window.location.search).get("demo") === "1") return true;
  return window.localStorage.getItem(DEMO_KEY) === "1";
}

/** Persists the flag and notifies same-tab subscribers. */
export function setDemoEnabled(enabled: boolean): void {
  window.localStorage.setItem(DEMO_KEY, enabled ? "1" : "0");
  window.dispatchEvent(new CustomEvent<boolean>(DEMO_EVENT, { detail: enabled }));
}

function isDemoEvent(event: Event): event is CustomEvent<boolean> {
  return event.type === DEMO_EVENT;
}

/** Subscribes to same-tab (`agentj:demo-updated`) and cross-tab (`storage`) changes; returns unsubscribe. */
export function subscribeDemo(cb: (enabled: boolean) => void): () => void {
  const onUpdated = (event: Event) => {
    if (isDemoEvent(event)) cb(event.detail);
  };
  const onStorage = (event: StorageEvent) => {
    if (event.key === DEMO_KEY) cb(event.newValue === "1");
  };
  window.addEventListener(DEMO_EVENT, onUpdated);
  window.addEventListener("storage", onStorage);
  return () => {
    window.removeEventListener(DEMO_EVENT, onUpdated);
    window.removeEventListener("storage", onStorage);
  };
}

/** preview-01/06 任务 card: 2 done, 1 in progress, 1 pending (counter 2 / 4). */
export const DEMO_TODOS: TodoItem[] = [
  { id: "1", content: "解析现有 App 布局", status: "completed" },
  { id: "2", content: "定义三栏 grid 与宽度 token", status: "completed" },
  { id: "3", content: "实现 SplitPane 拖拽与持久化", status: "in_progress" },
  { id: "4", content: "补充响应式与键盘可达性", status: "pending" },
];

/** preview-02/04 文件 tab tree; hidden children keep the footer at 10 个文件. */
export const DEMO_FILE_TREE: FileNode[] = [
  {
    name: "agentj-ui/",
    path: "agentj-ui",
    kind: "dir",
    children: [
      {
        name: "apps/",
        path: "agentj-ui/apps",
        kind: "dir",
        children: [
          {
            name: "web/",
            path: "agentj-ui/apps/web",
            kind: "dir",
            children: [
              {
                name: "src/",
                path: "agentj-ui/apps/web/src",
                kind: "dir",
                children: [
                  { name: "App.tsx", path: "agentj-ui/apps/web/src/App.tsx", kind: "file", git: "M" },
                  {
                    name: "UnifiedSidebar.tsx",
                    path: "agentj-ui/apps/web/src/UnifiedSidebar.tsx",
                    kind: "file",
                    git: "A",
                  },
                  { name: "RunPanel.tsx", path: "agentj-ui/apps/web/src/RunPanel.tsx", kind: "file", git: "A" },
                ],
              },
            ],
          },
          {
            name: "tui/",
            path: "agentj-ui/apps/tui",
            kind: "dir",
            children: [
              {
                name: "src/",
                path: "agentj-ui/apps/tui/src",
                kind: "dir",
                children: [{ name: "app.tsx", path: "agentj-ui/apps/tui/src/app.tsx", kind: "file", git: null }],
              },
            ],
          },
          {
            name: "desktop/",
            path: "agentj-ui/apps/desktop",
            kind: "dir",
            children: [
              {
                name: "src-tauri/",
                path: "agentj-ui/apps/desktop/src-tauri",
                kind: "dir",
                children: [
                  {
                    name: "tauri.conf.json",
                    path: "agentj-ui/apps/desktop/src-tauri/tauri.conf.json",
                    kind: "file",
                    git: null,
                  },
                ],
              },
            ],
          },
        ],
      },
      {
        name: "packages/",
        path: "agentj-ui/packages",
        kind: "dir",
        children: [
          {
            name: "sdk/",
            path: "agentj-ui/packages/sdk",
            kind: "dir",
            children: [{ name: "client.ts", path: "agentj-ui/packages/sdk/client.ts", kind: "file", git: null }],
          },
          {
            name: "tokens/",
            path: "agentj-ui/packages/tokens",
            kind: "dir",
            children: [
              { name: "palette.ts", path: "agentj-ui/packages/tokens/palette.ts", kind: "file", git: null },
            ],
          },
        ],
      },
    ],
  },
  {
    name: "crates/",
    path: "crates",
    kind: "dir",
    children: [{ name: "main.rs", path: "crates/main.rs", kind: "file", git: "U" }],
  },
  {
    name: "docs/",
    path: "docs",
    kind: "dir",
    children: [
      {
        name: "specs/",
        path: "docs/specs",
        kind: "dir",
        children: [{ name: "ipc-protocol.md", path: "docs/specs/ipc-protocol.md", kind: "file", git: null }],
      },
    ],
  },
  { name: "README.md", path: "README.md", kind: "file", git: null },
];

/** preview-02 App.tsx unified diff, copied line-for-line from mockup-files.html. */
const DEMO_APP_DIFF = [
  "  return (",
  '-   <div className="layout">',
  "+   <SplitPane left={256} right={320}>",
  "+     <Sidebar /><Main /><Detail />",
  "+   </SplitPane>",
  "  )",
].join("\n");

/** preview-01/02 文件变更 card: add SplitPane.tsx, modify App.tsx (diff) and tokens/palette.ts. */
export const DEMO_CHANGES: FileChangeRow[] = [
  { path: "SplitPane.tsx", op: "add", additions: 86, deletions: null, diff: null, toolCallId: null },
  { path: "App.tsx", op: "modify", additions: 24, deletions: 6, diff: DEMO_APP_DIFF, toolCallId: null },
  { path: "tokens/palette.ts", op: "modify", additions: 3, deletions: 1, diff: null, toolCallId: null },
];

/** preview-06 question card; option 2 is pre-selected in the mockup. */
export const DEMO_QUESTION: DemoQuestion = {
  id: "scope",
  question: "请选择本次重构的范围？",
  options: [
    { id: "ui", label: "仅前端三栏布局" },
    { id: "full", label: "三栏 + 协议扩展", hint: "推荐" },
    { id: "all", label: "全部（含 TUI）", hint: "范围较大" },
  ],
};

/** preview-06 answered record shown in the feed above the composer. */
export const DEMO_QA_RECORD: DemoQaRecord = {
  id: "qa-demo-1",
  question: "是否同时更新 TUI？",
  answer: "暂时不动 TUI",
  note: "已按此决定继续执行。",
};
