import { useEffect, useRef, useState } from "react";
import type { ChangeEvent, KeyboardEvent as ReactKeyboardEvent, ReactNode } from "react";
import OptionPopover from "./OptionPopover";
import { loadComposerPrefs, saveComposerPrefs } from "../layoutStore";
import type { ComposerAgent, ComposerPrefs } from "../layoutStore";
import { loadCurrentProject, loadProjects } from "../projectStore";
import "./options.css";

type OpenId = "model" | "agent" | "entropy" | null;

type ModelEntry = { name: string; cap: string };
type AgentEntry = { id: ComposerAgent; label: string; cap: string };
type EntropyPreset = { value: number; label: string; short: string };

export const MODEL_LIST: ModelEntry[] = [{ name: "deepseek-chat", cap: "128k 上下文 · 工具调用" }];

export const AGENT_LIST: AgentEntry[] = [
  { id: "build", label: "Build", cap: "默认 · 读写工具全开" },
  { id: "plan", label: "Plan", cap: "只读 · 先出方案不落盘" },
  { id: "explore", label: "Explore", cap: "只读 · 大范围检索" },
];

const ENTROPY_PRESETS: EntropyPreset[] = [
  { value: 0.2, label: "低温 · 精确", short: "低温" },
  { value: 0.7, label: "均衡 · 推荐", short: "均衡" },
  { value: 1.2, label: "高温 · 发散", short: "高温" },
];

export type ComposerOptionsProps = {
  model?: string | null;
  children?: ReactNode;
};

function agentIndex(agent: ComposerAgent): number {
  const index = AGENT_LIST.findIndex((entry) => entry.id === agent);
  return index < 0 ? 0 : index;
}

function entropyMatches(preset: EntropyPreset, entropy: number): boolean {
  return Math.abs(preset.value - entropy) < 1e-9;
}

function entropyIndex(entropy: number): number {
  const index = ENTROPY_PRESETS.findIndex((entry) => entropyMatches(entry, entropy));
  return index < 0 ? 0 : index;
}

function entropyShortLabel(entropy: number): string {
  const preset = ENTROPY_PRESETS.find((entry) => entropyMatches(entry, entropy));
  return preset ? preset.short : "自定义";
}

/**
 * Composer toolbar: model / agent / entropy selectors with exactly one popover
 * open at a time. Preferences persist through layoutStore and the trigger label
 * updates immediately. `children` is the right-aligned slot (RunPanel sends the
 * circular send button in T19).
 */
export default function ComposerOptions({ model = null, children }: ComposerOptionsProps) {
  const [prefs, setPrefs] = useState<ComposerPrefs>(() => loadComposerPrefs());
  const [openId, setOpenId] = useState<OpenId>(null);
  const [highlight, setHighlight] = useState(0);

  const toolsRef = useRef<HTMLDivElement>(null);
  const modelRef = useRef<HTMLButtonElement>(null);
  const agentRef = useRef<HTMLButtonElement>(null);
  const entropyRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    const applyFor = (id: string) => {
      const project = loadProjects().find((p) => p.id === id);
      if (!project) return;
      const patch: Partial<ComposerPrefs> = {};
      if (project.defaultModel) patch.model = project.defaultModel;
      if (project.defaultAgent) patch.agent = project.defaultAgent as ComposerAgent;
      if (Object.keys(patch).length === 0) return;
      setPrefs((prev) => ({ ...prev, ...patch }));
      saveComposerPrefs(patch);
    };
    applyFor(loadCurrentProject());
    const onCurrent = (event: Event) => {
      const id = (event as CustomEvent<{ id?: string }>).detail?.id;
      if (typeof id === "string") applyFor(id);
    };
    window.addEventListener("agentj:current-project-updated", onCurrent);
    return () => window.removeEventListener("agentj:current-project-updated", onCurrent);
  }, []);

  const modelName = model ?? prefs.model;
  const triggerRefs = { model: modelRef, agent: agentRef, entropy: entropyRef };

  const closePopover = (id: Exclude<OpenId, null>) => {
    setOpenId(null);
    triggerRefs[id].current?.focus();
  };

  const openPanel = (id: Exclude<OpenId, null>, initial: number) => {
    if (openId === id) {
      closePopover(id);
      return;
    }
    setHighlight(initial);
    setOpenId(id);
  };

  const selectOption = (id: Exclude<OpenId, null>, index: number) => {
    if (id === "model") {
      const entry = MODEL_LIST[index];
      if (!entry) return;
      setPrefs({ ...prefs, model: entry.name });
      saveComposerPrefs({ model: entry.name });
    } else if (id === "agent") {
      const entry = AGENT_LIST[index];
      if (!entry) return;
      setPrefs({ ...prefs, agent: entry.id });
      saveComposerPrefs({ agent: entry.id });
    } else {
      const preset = ENTROPY_PRESETS[index];
      if (!preset) return;
      setPrefs({ ...prefs, entropy: preset.value });
      saveComposerPrefs({ entropy: preset.value });
    }
    closePopover(id);
  };

  const optionCount = (id: Exclude<OpenId, null>): number => {
    if (id === "model") return MODEL_LIST.length;
    if (id === "agent") return AGENT_LIST.length;
    return ENTROPY_PRESETS.length;
  };

  const handleKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (!openId || (event.target as HTMLElement).tagName === "INPUT") return;
    const count = optionCount(openId);
    if (count === 0) return;
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setHighlight((current) => (current + 1) % count);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setHighlight((current) => (current - 1 + count) % count);
    } else if (event.key === "Enter") {
      event.preventDefault();
      selectOption(openId, highlight);
    }
  };

  const handleSlider = (event: ChangeEvent<HTMLInputElement>) => {
    const value = Math.round(Number(event.target.value) * 10) / 10;
    setPrefs({ ...prefs, entropy: value });
    saveComposerPrefs({ entropy: value });
  };

  return (
    <div className="comp-tools" ref={toolsRef}>
      <div className="sel-wrap">
        <button
          type="button"
          id="composer-model-trigger"
          ref={modelRef}
          className="sel"
          aria-haspopup="listbox"
          aria-expanded={openId === "model"}
          onClick={() => openPanel("model", modelName ? MODEL_LIST.findIndex((e) => e.name === modelName) : 0)}
        >
          <span className="g">◆</span>
          {modelName ?? "模型"}
          <span className="cv">⌄</span>
        </button>
        <OptionPopover
          open={openId === "model"}
          onClose={() => closePopover("model")}
          anchorRef={modelRef}
          className="pop pop-left pop-model"
          labelledBy="composer-model-title"
          role="listbox"
          tabIndex={-1}
          onKeyDown={handleKeyDown}
        >
          <div className="pop-title" id="composer-model-title">选择模型</div>
          {MODEL_LIST.map((entry, index) => {
            const selected = modelName === entry.name;
            return (
              <div
                key={entry.name}
                role="option"
                aria-selected={selected}
                className={`pop-item${selected ? " sel-item" : ""}${highlight === index ? " hl" : ""}`}
                onClick={() => selectOption("model", index)}
              >
                <div>
                  <div className="nm">{entry.name}</div>
                  <div className="cap">{entry.cap}</div>
                </div>
                {selected && <span className="tick">✓</span>}
              </div>
            );
          })}
        </OptionPopover>
      </div>

      <div className="sel-wrap">
        <button
          type="button"
          id="composer-agent-trigger"
          ref={agentRef}
          className="sel"
          aria-haspopup="listbox"
          aria-expanded={openId === "agent"}
          onClick={() => openPanel("agent", agentIndex(prefs.agent))}
        >
          <span className="g">⚙</span>
          {AGENT_LIST[agentIndex(prefs.agent)].label}
          <span className="cv">⌄</span>
        </button>
        <OptionPopover
          open={openId === "agent"}
          onClose={() => closePopover("agent")}
          anchorRef={agentRef}
          className="pop pop-left pop-agent"
          labelledBy="composer-agent-title"
          role="listbox"
          tabIndex={-1}
          onKeyDown={handleKeyDown}
        >
          <div className="pop-title" id="composer-agent-title">选择 Agent</div>
          {AGENT_LIST.map((entry, index) => {
            const selected = prefs.agent === entry.id;
            return (
              <div
                key={entry.id}
                role="option"
                aria-selected={selected}
                className={`pop-item${selected ? " sel-item" : ""}${highlight === index ? " hl" : ""}`}
                onClick={() => selectOption("agent", index)}
              >
                <div>
                  <div className="nm">{entry.label}</div>
                  <div className="cap">{entry.cap}</div>
                </div>
                {selected && <span className="tick">✓</span>}
              </div>
            );
          })}
          <div className="pop-hint">Agent 决定可用工具与权限范围。</div>
        </OptionPopover>
      </div>

      <div className="sel-wrap">
        <button
          type="button"
          id="composer-entropy-trigger"
          ref={entropyRef}
          className="sel"
          aria-haspopup="listbox"
          aria-expanded={openId === "entropy"}
          onClick={() => openPanel("entropy", entropyIndex(prefs.entropy))}
        >
          <span className="g">∿</span>
          {entropyShortLabel(prefs.entropy)}
          <span className="cv">⌄</span>
        </button>
        <OptionPopover
          open={openId === "entropy"}
          onClose={() => closePopover("entropy")}
          anchorRef={entropyRef}
          className="pop pop-right pop-ent"
          labelledBy="composer-entropy-title"
          role="listbox"
          tabIndex={-1}
          onKeyDown={handleKeyDown}
        >
          <div className="pop-title" id="composer-entropy-title">熵变</div>
          {ENTROPY_PRESETS.map((preset, index) => {
            const selected = entropyMatches(preset, prefs.entropy);
            return (
              <div
                key={preset.value}
                role="option"
                aria-selected={selected}
                className={`pop-row${selected ? " sel-item" : ""}${highlight === index ? " hl" : ""}`}
                onClick={() => selectOption("entropy", index)}
              >
                <span className="nm2">{preset.label}</span>
                <span className="v">{preset.value.toFixed(1)}</span>
                {selected && <span className="tick">✓</span>}
              </div>
            );
          })}
          <div className="pop-sep" />
          <div className="pop-row">
            <span className="nm2">自定义</span>
            <span className="v">{prefs.entropy.toFixed(1)}</span>
          </div>
          <input
            type="range"
            className="slider"
            aria-label="自定义熵值"
            min={0}
            max={2}
            step={0.1}
            value={prefs.entropy}
            onChange={handleSlider}
          />
          <div className="pop-hint">数值越高输出越发散，越低越确定。</div>
        </OptionPopover>
      </div>

      <span className="spacer" />
      {children}
    </div>
  );
}
