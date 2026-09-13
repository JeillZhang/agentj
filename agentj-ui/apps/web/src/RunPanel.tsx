import { useEffect, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Check, ChevronDown, Clock, Folder, Minus, X } from "lucide-react";
import { METHODS, parseProgress, RpcClient } from "@agentj/sdk";
import type { ProgressFrame } from "@agentj/sdk";
import ComposerOptions from "./components/ComposerOptions";
import AskQuestionCard, { type AskAnswer } from "./components/AskQuestionCard";
import { DEMO_QA_RECORD, DEMO_QUESTION, isDemoEnabled, subscribeDemo } from "./demoData";
import { loadAskCollapsed, saveAskCollapsed } from "./layoutStore";
import { createProject, loadCurrentProject, loadProjects, setCurrentProject } from "./projectStore";

export type RunEntry =
  | { kind: "user"; text: string }
  | { kind: "ai"; text: string }
  | { kind: "tool"; id: string; name: string; args: unknown; state: "running" | "ok" | "denied" | "failed"; output?: string }
  | { kind: "usage"; prompt_tokens: number; completion_tokens: number }
  | { kind: "qa"; id: string; question: string; answer: string }
  | { kind: "error"; text: string };

export function reduceRunFeed(entries: RunEntry[], frame: ProgressFrame): RunEntry[] {
  const last = entries[entries.length - 1];
  switch (frame.kind) {
    case "text":
      if (last?.kind === "ai") {
        return [...entries.slice(0, -1), { kind: "ai", text: last.text + frame.text }];
      }
      return [...entries, { kind: "ai", text: frame.text }];
    case "tool_call":
      return [
        ...entries,
        { kind: "tool", id: frame.id, name: frame.name, args: frame.arguments, state: "running" },
      ];
    case "tool_result": {
      const settle = (t: Extract<RunEntry, { kind: "tool" }>, j: number): RunEntry[] => {
        const state: "ok" | "failed" = frame.content.startsWith("Error:") ? "failed" : "ok";
        const next: RunEntry = { ...t, state, output: frame.content };
        return [...entries.slice(0, j), next, ...entries.slice(j + 1)];
      };
      const i = entries.findIndex((e) => e.kind === "tool" && e.id === frame.call_id);
      if (i >= 0) {
        const t = entries[i] as Extract<RunEntry, { kind: "tool" }>;
        return t.state === "running" ? settle(t, i) : entries;
      }
      const j = entries.findLastIndex((e) => e.kind === "tool" && e.state === "running");
      if (j < 0) return entries;
      return settle(entries[j] as Extract<RunEntry, { kind: "tool" }>, j);
    }
    case "permission": {
      if (frame.decision !== "deny") return entries;
      const j = entries.findLastIndex(
        (e) => e.kind === "tool" && e.state === "running" && e.name === frame.tool,
      );
      if (j < 0) return entries;
      const t = entries[j] as Extract<RunEntry, { kind: "tool" }>;
      return [...entries.slice(0, j), { ...t, state: "denied" }, ...entries.slice(j + 1)];
    }
    case "usage": {
      const u = { kind: "usage" as const, prompt_tokens: frame.usage.prompt_tokens, completion_tokens: frame.usage.completion_tokens };
      return last?.kind === "usage" ? [...entries.slice(0, -1), u] : [...entries, u];
    }
    case "unknown":
      return entries;
  }
}

export function failRun(entries: RunEntry[], message: string): RunEntry[] {
  const settled = entries.map((e) =>
    e.kind === "tool" && e.state === "running" ? { ...e, state: "failed" as const } : e,
  );
  return [...settled, { kind: "error", text: message }];
}

export function fmtDuration(ms: number): string {
  return ms < 1000 ? `${Math.round(ms)}ms` : `${(ms / 1000).toFixed(2)}s`;
}

function demoQaEntry(): Extract<RunEntry, { kind: "qa" }> {
  return {
    kind: "qa",
    id: DEMO_QA_RECORD.id,
    question: DEMO_QA_RECORD.question,
    answer: DEMO_QA_RECORD.answer,
  };
}

function withDemoSeed(entries: RunEntry[]): RunEntry[] {
  if (entries.some((e) => e.kind === "qa" && e.id === DEMO_QA_RECORD.id)) return entries;
  return [...entries, demoQaEntry()];
}

type Props = {
  client: RpcClient | null;
  activeSessionId: string | null;
  initialEntries?: RunEntry[];
  pendingTool?: string;
  model?: string;
  elapsedMs?: number;
  wsUrl?: string;
  modules?: { id: string; state: string }[];
  status?: string;
  unreachable?: boolean;
};

export function toolGlyph(entry: Extract<RunEntry, { kind: "tool" }>, pending: boolean) {
  if (entry.state === "ok") return <Check size={16} aria-hidden="true" />;
  if (entry.state === "denied" || entry.state === "failed") return <X size={16} aria-hidden="true" />;
  return pending ? <Clock size={16} aria-hidden="true" /> : <Minus size={16} aria-hidden="true" />;
}

export function ToolCard({
  entry,
  pending,
}: {
  entry: Extract<RunEntry, { kind: "tool" }>;
  pending: boolean;
}) {
  const [open, setOpen] = useState(false);
  const lines = (entry.output ?? "").split("\n");
  const folded = !open && lines.length > 3;
  const shown = folded ? lines.slice(0, 3).join("\n") : entry.output ?? "";
  const state = pending ? "pending" : entry.state;
  return (
    <section className="aj-tool-card" aria-label={entry.name}>
      <span className="aj-tool-dot" data-state={state} aria-hidden="true" />
      <span className="aj-tool-status" data-state={state}>{toolGlyph(entry, pending)}</span>
      <span className="aj-tool-name">{entry.name}</span>
      {entry.output !== undefined && (
        <button type="button" aria-label={open ? "collapse" : "expand"} onClick={() => setOpen(!open)}>
          {open ? "collapse" : "expand"}
        </button>
      )}
      {shown !== "" && <pre className="aj-tool-output">{shown}{folded ? " …" : ""}</pre>}
    </section>
  );
}

export function ConversationFeed({
  entries,
  pendingTool,
  model,
  elapsedMs,
}: {
  entries: RunEntry[];
  pendingTool?: string;
  model?: string;
  elapsedMs?: number;
}) {
  const endRef = useRef<HTMLDivElement | null>(null);
  const [pinned, setPinned] = useState(true);
  useEffect(() => {
    if (pinned) (endRef.current as unknown as { scrollIntoView?: (o?: unknown) => void })?.scrollIntoView?.({ block: "end" });
  });
  return (
    <div
      className="aj-conv"
      role="feed"
      aria-label="progress feed"
      onScroll={(e) => {
        const el = e.currentTarget;
        setPinned(el.scrollHeight - el.scrollTop - el.clientHeight < 40);
      }}
    >
      {entries.map((entry, i) => {
        switch (entry.kind) {
          case "user":
            return (
              <div key={i} className="aj-bubble-user aj-bubble-user--right">{entry.text}</div>
            );
          case "ai":
            return (
              <div key={i} className="aj-ai">
                <ReactMarkdown remarkPlugins={[remarkGfm]}>{entry.text}</ReactMarkdown>
              </div>
            );
          case "tool":
            return (
              <ToolCard key={i} entry={entry} pending={pendingTool === entry.name && entry.state === "running"} />
            );
          case "usage": {
            const isLast = i === entries.length - 1;
            return (
              <div key={i} className="aj-meta">
                {model !== undefined && <span className="aj-meta-item">{model}</span>}
                {isLast && elapsedMs !== undefined && (
                  <span className="aj-meta-item">{fmtDuration(elapsedMs)}</span>
                )}
                <span className="aj-meta-item">
                  in={entry.prompt_tokens} out={entry.completion_tokens}
                </span>
              </div>
            );
          }
          case "qa":
            return (
              <div key={i} className="qa">
                <div className="qa-head">
                  <span className="qa-badge" aria-hidden="true">?</span>
                  <span className="qa-title">模型提问</span>
                  <span className="qa-fold">已折叠 ⌄</span>
                </div>
                <div className="qa-q">{entry.question}</div>
                <div className="qa-a">→ {entry.answer}</div>
                <div className="qa-note">已按此决定继续执行。</div>
              </div>
            );
          case "error":
            return (
              <div key={i} className="aj-error-row" role="alert">
                {entry.text}
              </div>
            );
        }
      })}
      <div ref={endRef} />
    </div>
  );
}

const MOCK_FILES = [
  "src/index.css",
  "src/App.tsx",
  "src/RunPanel.tsx",
  "src/components/composer.css",
  "src/projectStore.ts",
  "src/main.tsx",
  "README.md",
  "agentj-ui/apps/web/package.json",
];

const SLASH_COMMANDS = ["/help", "/clear", "/new", "/model"];

function getAtPrefix(value: string): string | null {
  const m = value.match(/@([^\s]*)$/);
  if (m) return m[1];
  // also handle @ in middle before cursor end - last @
  const idx = value.lastIndexOf("@");
  if (idx === -1) return null;
  const after = value.slice(idx + 1);
  if (after.includes(" ")) return null;
  // if @ is followed by file chars, treat as trigger
  // trigger if @ present and no space after it till end (or with slash)
  if (value.slice(idx).match(/^@[^\s]*$/)) return after;
  return null;
}

export default function RunPanel({
  client,
  activeSessionId,
  initialEntries = [],
  pendingTool,
  model,
}: Props) {
  const [entries, setEntries] = useState<RunEntry[]>(initialEntries);
  const [elapsedMs, setElapsedMs] = useState<number | undefined>(undefined);
  const [draft, setDraft] = useState("");
  const busy = useRef(false);
  const [isBusy, setIsBusy] = useState(false);

  const [demo, setDemo] = useState(isDemoEnabled);
  const [askVisible, setAskVisible] = useState(isDemoEnabled);
  const [askCollapsed, setAskCollapsed] = useState(loadAskCollapsed);
  const [chosenLabel, setChosenLabel] = useState<string | undefined>(undefined);
  const askSubmitted = useRef(false);
  const demoRef = useRef(demo);
  demoRef.current = demo;

  useEffect(() => subscribeDemo(setDemo), []);

  useEffect(() => {
    setAskVisible(demo);
  }, [demo]);

  useEffect(() => {
    if (initialEntries.length === 0) return;
    setEntries((prev) => {
      if (prev === initialEntries) return prev;
      return demoRef.current ? withDemoSeed(initialEntries) : initialEntries;
    });
  }, [initialEntries]);

  useEffect(() => {
    if (!demo) return;
    setEntries((prev) => withDemoSeed(prev));
  }, [demo]);

  // composer header state
  const [projects, setProjects] = useState(() => {
    try { return loadProjects(); } catch { return [{ id: "default", name: "Inbox", createdAt: 0 }]; }
  });
  const [currentId, setCurrentId] = useState(() => {
    try { return loadCurrentProject(); } catch { return "default"; }
  });
  const currentProjectName = projects.find((p) => p.id === currentId)?.name ?? "Inbox";
  const [pickerOpen, setPickerOpen] = useState(false);
  const [pickerIndex, setPickerIndex] = useState(0);
  const [atFilter, setAtFilter] = useState<string | null>(null);
  const [atIndex, setAtIndex] = useState(0);
  const [slashOpen, setSlashOpen] = useState(false);
  const [slashIndex, setSlashIndex] = useState(0);

  const pickerRef = useRef<HTMLDivElement | null>(null);
  const atOpen = atFilter !== null;

  const filteredFiles = atFilter !== null
    ? MOCK_FILES.filter((f) => f.toLowerCase().includes(atFilter.toLowerCase())).slice(0, 8)
    : [];

  const filteredSlash = SLASH_COMMANDS;

  useEffect(() => {
    try { setProjects(loadProjects()); } catch {}
  }, [pickerOpen]);

  useEffect(() => {
    if (!client) return;
    const handler = (method: string, params: unknown) => {
      if (method !== METHODS.progress) return;
      const f = parseProgress(params);
      if (f.kind === "unknown") {
        console.debug("run:ignored-frame", params);
        return;
      }
      setEntries((prev) => reduceRunFeed(prev, f));
    };
    client.setNotificationHandler(handler);
    return () => {
      client.removeNotificationHandler?.(handler);
    };
  }, [client]);

  // close on outside click
  useEffect(() => {
    if (!pickerOpen) return;
    const onDown = (e: MouseEvent) => {
      const t = e.target as HTMLElement;
      if (pickerRef.current && !pickerRef.current.contains(t)) {
        setPickerOpen(false);
      }
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [pickerOpen]);

  function handleDraftChange(v: string) {
    setDraft(v);
    const pref = getAtPrefix(v);
    if (pref !== null) {
      setAtFilter(pref);
      setAtIndex(0);
      setSlashOpen(false);
    } else {
      setAtFilter(null);
    }
    if (v.startsWith("/")) {
      setSlashOpen(true);
      setSlashIndex(0);
      if (pref === null) setAtFilter(null);
    } else {
      setSlashOpen(false);
    }
  }

  function insertAtFile(file: string) {
    const idx = draft.lastIndexOf("@");
    if (idx === -1) return;
    const next = draft.slice(0, idx) + "@" + file + " ";
    setDraft(next);
    setAtFilter(null);
    setSlashOpen(false);
  }

  function insertSlash(cmd: string) {
    setDraft(cmd + " ");
    setSlashOpen(false);
    setAtFilter(null);
  }

  function toggleAskCollapsed() {
    const next = !askCollapsed;
    saveAskCollapsed(next);
    setAskCollapsed(next);
  }

  function handleAskSubmit({ optionId, other }: AskAnswer) {
    if (askSubmitted.current) return;
    askSubmitted.current = true;
    const option = DEMO_QUESTION.options.find((o) => o.id === optionId);
    const answer = option?.label ?? other;
    setChosenLabel(answer);
    setAskVisible(false);
    setEntries((prev) => [
      ...prev,
      { kind: "qa", id: `qa-${prev.length + 1}`, question: DEMO_QUESTION.question, answer },
    ]);
  }

  function handleAskSkip() {
    setAskVisible(false);
  }

  async function submit(text: string) {
    if (!activeSessionId || busy.current || text.trim() === "" || !client) return;
    busy.current = true;
    setIsBusy(true);
    setElapsedMs(undefined);
    const startedAt = performance.now();
    setEntries((prev) => [...prev, { kind: "user", text }]);
    let usage: { usage?: { prompt_tokens?: number; completion_tokens?: number } } | null = null;
    try {
      try {
        usage = (await client.request(METHODS.sessionRun, {
          session_id: activeSessionId,
          content: text,
        })) as { usage?: { prompt_tokens?: number; completion_tokens?: number } } | null;
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        setEntries((prev) => failRun(prev, message));
      }
    } finally {
      setElapsedMs(performance.now() - startedAt);
      busy.current = false;
      setIsBusy(false);
    }
    const pt = usage?.usage?.prompt_tokens;
    const ct = usage?.usage?.completion_tokens;
    if (typeof pt === "number" && typeof ct === "number") {
      setEntries((prev) => {
        const last = prev[prev.length - 1];
        if (last?.kind === "usage") return prev;
        return [...prev, { kind: "usage", prompt_tokens: pt, completion_tokens: ct }];
      });
    }
  }

  return (
    <section aria-label="run" className="aj-card">
      <ConversationFeed
        entries={entries}
        pendingTool={pendingTool}
        model={model}
        elapsedMs={elapsedMs}
      />
      <div className="aj-composer-card">
        <div className="aj-composer__header">
          <div className="aj-composer__picker-wrap" ref={pickerRef}>
            <button
              type="button"
              aria-label="project picker"
              aria-expanded={pickerOpen}
              aria-haspopup="listbox"
              className="aj-composer__project-btn"
              onClick={() => {
                setPickerOpen((o) => !o);
                setPickerIndex(0);
                try { setProjects(loadProjects()); } catch {}
              }}
              onKeyDown={(e) => {
                if (!pickerOpen && (e.key === "ArrowDown" || e.key === "Enter" || e.key === " ")) {
                  e.preventDefault();
                  setPickerOpen(true);
                  setPickerIndex(0);
                  return;
                }
                if (!pickerOpen) return;
                const count = projects.length + 1;
                if (e.key === "ArrowDown") {
                  e.preventDefault();
                  setPickerIndex((i) => (i + 1) % count);
                } else if (e.key === "ArrowUp") {
                  e.preventDefault();
                  setPickerIndex((i) => (i - 1 + count) % count);
                } else if (e.key === "Enter") {
                  e.preventDefault();
                  if (pickerIndex < projects.length) {
                    const p = projects[pickerIndex];
                    setCurrentProject(p.id);
                    setCurrentId(p.id);
                  } else {
                    const created = createProject("New Project");
                    setCurrentProject(created.id);
                    setCurrentId(created.id);
                    setProjects(loadProjects());
                  }
                  setPickerOpen(false);
                } else if (e.key === "Escape") {
                  e.preventDefault();
                  setPickerOpen(false);
                }
              }}
            >
              <Folder size={16} aria-hidden="true" />
              <span>{currentProjectName}</span>
              <ChevronDown size={16} aria-hidden="true" />
            </button>
            {pickerOpen && (
              <ul role="listbox" className="aj-composer__picker-list" aria-label="project list">
                {projects.map((p, idx) => (
                  <li
                    key={p.id}
                    role="option"
                    aria-selected={pickerIndex === idx}
                    data-focused={pickerIndex === idx ? "true" : undefined}
                    tabIndex={-1}
                    className="aj-composer__picker-option"
                    onClick={() => {
                      setCurrentProject(p.id);
                      setCurrentId(p.id);
                      setPickerOpen(false);
                    }}
                    onMouseEnter={() => setPickerIndex(idx)}
                  >
                    {p.name}
                  </li>
                ))}
                <li
                  role="option"
                  aria-selected={pickerIndex === projects.length}
                  data-focused={pickerIndex === projects.length ? "true" : undefined}
                  tabIndex={-1}
                  className="aj-composer__picker-option aj-composer__picker-option--new"
                  onClick={() => {
                    const created = createProject("New Project");
                    setCurrentProject(created.id);
                    setCurrentId(created.id);
                    setProjects(loadProjects());
                    setPickerOpen(false);
                  }}
                  onMouseEnter={() => setPickerIndex(projects.length)}
                >
                  New Project
                </li>
              </ul>
            )}
          </div>
          {model !== undefined && model !== null && model !== "" && (
            <span className="aj-chip aj-chip--model">{model}</span>
          )}
        </div>

        {askVisible && (
          <AskQuestionCard
            question={DEMO_QUESTION}
            collapsed={askCollapsed}
            selectedLabel={chosenLabel}
            onToggleCollapsed={toggleAskCollapsed}
            onSubmit={handleAskSubmit}
            onSkip={handleAskSkip}
          />
        )}

        <div style={{ position: "relative" }}>
          <label htmlFor="aj-prompt" className="aj-visually-hidden">prompt</label>
          <textarea
            id="aj-prompt"
            className="aj-input"
            value={draft}
            placeholder={activeSessionId ? "继续追问…" : "先新建或选择一个会话"}
            disabled={!activeSessionId || isBusy}
            onChange={(e) => handleDraftChange(e.target.value)}
            onKeyDown={(e) => {
              // @ completion nav
              if (atOpen && filteredFiles.length > 0) {
                if (e.key === "ArrowDown") {
                  e.preventDefault();
                  setAtIndex((i) => (i + 1) % filteredFiles.length);
                  return;
                }
                if (e.key === "ArrowUp") {
                  e.preventDefault();
                  setAtIndex((i) => (i - 1 + filteredFiles.length) % filteredFiles.length);
                  return;
                }
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  insertAtFile(filteredFiles[atIndex]);
                  return;
                }
                if (e.key === "Escape") {
                  e.preventDefault();
                  setAtFilter(null);
                  return;
                }
              }
              if (slashOpen) {
                if (e.key === "ArrowDown") {
                  e.preventDefault();
                  setSlashIndex((i) => (i + 1) % filteredSlash.length);
                  return;
                }
                if (e.key === "ArrowUp") {
                  e.preventDefault();
                  setSlashIndex((i) => (i - 1 + filteredSlash.length) % filteredSlash.length);
                  return;
                }
                if (e.key === "Enter") {
                  e.preventDefault();
                  insertSlash(filteredSlash[slashIndex]);
                  return;
                }
                if (e.key === "Escape") {
                  e.preventDefault();
                  setSlashOpen(false);
                  return;
                }
              }
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                if (busy.current) return;
                submit(draft);
                setDraft("");
                setAtFilter(null);
                setSlashOpen(false);
              }
            }}
          />
          {atOpen && (
            <ul role="listbox" aria-label="file completion" className="aj-composer__at-list">
              {filteredFiles.length === 0 ? (
                <li role="option" aria-selected={false} className="aj-composer__at-option">No matches</li>
              ) : (
                filteredFiles.map((f, idx) => (
                  <li
                    key={f}
                    role="option"
                    aria-selected={idx === atIndex}
                    data-focused={idx === atIndex ? "true" : undefined}
                    className="aj-composer__at-option"
                    onMouseEnter={() => setAtIndex(idx)}
                    onMouseDown={(e) => {
                      e.preventDefault();
                      insertAtFile(f);
                    }}
                  >
                    {f}
                  </li>
                ))
              )}
            </ul>
          )}
          {slashOpen && !atOpen && (
            <ul role="listbox" aria-label="slash commands" className="aj-composer__at-list">
              {filteredSlash.map((c, idx) => (
                <li
                  key={c}
                  role="option"
                  aria-selected={idx === slashIndex}
                  data-focused={idx === slashIndex ? "true" : undefined}
                  className="aj-composer__at-option"
                  onMouseEnter={() => setSlashIndex(idx)}
                  onMouseDown={(e) => {
                    e.preventDefault();
                    insertSlash(c);
                  }}
                >
                  {c}
                </li>
              ))}
            </ul>
          )}
        </div>
        <ComposerOptions model={model}>
          <button
            type="button"
            className="send aj-button--send"
            aria-label="发送"
            disabled={!activeSessionId || isBusy}
            onClick={() => {
              if (busy.current) return;
              submit(draft);
              setDraft("");
              setAtFilter(null);
              setSlashOpen(false);
            }}
          >
            ↑
          </button>
        </ComposerOptions>
      </div>
    </section>
  );
}
