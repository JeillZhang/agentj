import { useRef, useState, useCallback, useEffect } from "react";
import { useKeyboard } from "@opentui/react";
import type { TextareaRenderable } from "@opentui/core";
import { completePath, getAtPrefix, insertCompletion, CompletionOverlay } from "./completion.tsx";

export type ComposerProps = {
  onSubmit: (text: string) => void;
  placeholder?: string;
  disabled?: boolean;
  files?: string[];
  onFocusSidebar?: () => void;
};

export type HistoryStore = {
  history: string[];
  addHistory: (v: string) => void;
  handleUp: () => void;
  handleDown: () => void;
  setIdx: (v: number | ((p: number) => number)) => void;
};

function useHistoryStore(ref: React.RefObject<TextareaRenderable | null>): HistoryStore {
  const [history, setHistory] = useState<string[]>([]);
  const [idx, setIdx] = useState<number>(-1);
  const [draft, setDraft] = useState<string>("");
  const addHistory = useCallback((v: string) => {
    setHistory((p) => {
      const n = [...p, v];
      return n.length > 100 ? n.slice(-100) : n;
    });
    setIdx(-1);
    setDraft("");
  }, []);
  const handleUp = useCallback(() => {
    if (history.length === 0) return;
    setIdx((prev) => {
      let next: number;
      if (prev === -1) {
        setDraft(ref.current?.plainText ?? "");
        next = history.length - 1;
      } else next = Math.max(0, prev - 1);
      const val = history[next];
      if (val !== undefined) ref.current?.setText(val);
      return next;
    });
  }, [history, ref]);
  const handleDown = useCallback(() => {
    setIdx((prev) => {
      if (prev === -1) return -1;
      if (prev >= history.length - 1) {
        ref.current?.setText(draft);
        return -1;
      }
      const next = prev + 1;
      const val = history[next];
      if (val !== undefined) ref.current?.setText(val);
      return next;
    });
  }, [history, draft, ref]);
  return { history, addHistory, handleUp, handleDown, setIdx };
}

function useAtCompletion(ref: React.RefObject<TextareaRenderable | null>, files?: string[]) {
  const [inputText, setInputText] = useState("");
  const [completionIdx, setCompletionIdx] = useState(0);
  const [dismissed, setDismissed] = useState(false);
  const prefix = getAtPrefix(inputText);
  const matches = prefix && !dismissed ? completePath(prefix, files ?? []) : [];
  const showCompletion = prefix !== null && matches.length > 0 && !dismissed;
  useEffect(() => {
    setCompletionIdx(0);
    setDismissed(false);
  }, [prefix]);
  const insertChosen = useCallback(
    (chosen: string) => {
      const cur = ref.current?.plainText ?? inputText;
      const next = insertCompletion(cur, chosen);
      ref.current?.setText(next);
      setInputText(next);
      setCompletionIdx(0);
      setDismissed(true);
    },
    [inputText, ref],
  );
  const dismiss = useCallback(() => setDismissed(true), []);
  return {
    inputText,
    setInputText,
    completionIdx,
    setCompletionIdx,
    prefix,
    matches,
    showCompletion,
    insertChosen,
    dismiss,
    setDismissed,
  };
}

type CompletionState = {
  show: boolean;
  matches: string[];
  idx: number;
  setIdx: (v: number | ((p: number) => number)) => void;
  insert: (chosen: string) => void;
  dismiss: () => void;
};

function useComposerKeys(
  ref: React.RefObject<TextareaRenderable | null>,
  disabled: boolean | undefined,
  onSubmit: (s: string) => void,
  store: HistoryStore,
  c: CompletionState | null,
  onFocusSidebar?: () => void,
) {
  useKeyboard((key) => {
    if (disabled) return;
    const n = key.name.toLowerCase();
    const seq = key.sequence ?? "";
    const isEnter = n === "return" || n === "enter" || seq === "\r" || seq === "\n";
    const isEsc = n === "escape" || seq === "\x1b";
    const isUp = n === "up" || seq === "\x1b[A";
    const isDown = n === "down" || seq === "\x1b[B";
    const isTab = n === "tab" || seq === "\t" || seq === "\x09";
    if (c?.show) {
      if (isUp) { c.setIdx((v) => Math.max(0, v - 1)); return; }
      if (isDown) { c.setIdx((v) => Math.min(c.matches.length - 1, v + 1)); return; }
      if (isTab || isEnter) { const ch = c.matches[c.idx]; if (ch) c.insert(ch); return; }
      if (isEsc) { c.dismiss(); return; }
    }
    if (isEnter) {
      if (key.ctrl || key.meta) return;
      const raw = ref.current?.plainText ?? "";
      const trimmed = raw.trim();
      if (trimmed.length === 0) return;
      store.addHistory(trimmed);
      onSubmit(trimmed);
      ref.current?.setText("");
      return;
    }
    if (isEsc) { ref.current?.setText(""); store.setIdx(-1); return; }
    if (isUp) { store.handleUp(); return; }
    if (isDown) store.handleDown();
    if (isTab) onFocusSidebar?.();
  });
}

export function Composer(props: ComposerProps): React.ReactNode {
  const { onSubmit, placeholder, disabled, files, onFocusSidebar } = props;
  const ref = useRef<TextareaRenderable>(null);
  const store = useHistoryStore(ref);
  const ac = useAtCompletion(ref, files);
  const cState: CompletionState | null = ac.showCompletion ? {
    show: true, matches: ac.matches, idx: ac.completionIdx,
    setIdx: ac.setCompletionIdx, insert: ac.insertChosen, dismiss: ac.dismiss,
  } : null;
  useComposerKeys(ref, disabled, onSubmit, store, cState, onFocusSidebar);
  const onTextareaSubmit = useCallback(() => {
    const raw = ref.current?.plainText ?? "";
    const trimmed = raw.trim();
    if (trimmed.length === 0) return;
    store.addHistory(trimmed);
    onSubmit(trimmed);
    ref.current?.setText("");
    ac.setInputText("");
  }, [onSubmit, store, ac]);
  const onKeyDown = useCallback(
    (e: { name: string; sequence?: string }) => {
      const n = (e.name ?? "").toLowerCase();
      const seq = e.sequence ?? "";
      if (n === "escape" || seq === "\u001b") {
        ref.current?.setText("");
        store.setIdx(-1);
        ac.setInputText("");
        ac.setDismissed(true);
      }
    },
    [store, ac],
  );
  const handleContentChange = useCallback(() => ac.setInputText(ref.current?.plainText ?? ""), [ac, ref]);
  return (
    <box flexDirection="column" padding={1} border>
      <textarea
        ref={ref}
        placeholder={placeholder ?? "Type prompt, Enter to send, Esc to clear"}
        focused={!disabled}
        onSubmit={onTextareaSubmit}
        onKeyDown={onKeyDown as unknown as never}
        onContentChange={handleContentChange as unknown as never}
      />
      {ac.showCompletion ? (
        <CompletionOverlay matches={ac.matches} selected={ac.completionIdx} onSelect={ac.insertChosen} />
      ) : null}
    </box>
  );
}
