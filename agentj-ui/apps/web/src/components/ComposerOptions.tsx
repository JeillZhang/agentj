import { useEffect, useRef, useState } from "react";
import type { ChangeEvent, KeyboardEvent as ReactKeyboardEvent, ReactNode } from "react";
import OptionPopover from "./OptionPopover";
import { COMPOSER_KEYS, loadComposerPrefs, saveComposerPrefs } from "../layoutStore";
import type { ComposerAgent, ComposerPermission, ComposerPrefs } from "../layoutStore";
import { loadCurrentProject, loadProjects } from "../projectStore";
import { modelKey, resolveModelMarker } from "@agentj/sdk";
import "./options.css";

type OpenId = "model" | "agent" | "entropy" | "permission" | null;

export type ModelEntry = { name: string; cap?: string; provider?: string };
type AgentEntry = { id: ComposerAgent; label: string; cap: string };
type EntropyPreset = { value: number; label: string; short: string };
type PermissionEntry = { id: ComposerPermission; label: string; cap: string };

/** Shown instead of model rows when the kernel served no model list (nothing selectable). */
export const NO_MODEL_HINT = "内核未返回模型列表";

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

export const PERMISSION_MODES: PermissionEntry[] = [
  { id: "ask", label: "逐次确认", cap: "每次工具调用弹窗确认（默认）" },
  { id: "bypass", label: "自动允许", cap: "自动放行审批请求，不再弹窗（内核强制拒绝仍生效）" },
];

export type ComposerOptionsProps = {
  model?: string | null;
  /** Kernel-served model list. Empty/absent means the kernel served none: the picker then
   * offers nothing to pick (a placeholder row is shown) — never a fabricated model name. */
  models?: ModelEntry[];
  /** Explicit user pick; wins over the kernel-default `model` for the trigger label. */
  selected?: ModelEntry | null;
  /** Called with the exact picked entry (provider tag included) on model selection. */
  onSelect?: (entry: ModelEntry) => void;
  collapsed?: boolean;
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

function permissionIndex(mode: ComposerPermission): number {
  const index = PERMISSION_MODES.findIndex((entry) => entry.id === mode);
  return index < 0 ? 0 : index;
}

/**
 * Composer toolbar: model / agent / entropy / permission selectors with exactly
 * one popover open at a time. Preferences persist through layoutStore and the trigger label
 * updates immediately. `children` is the right-aligned slot (RunPanel sends the
 * circular send button in T19).
 */
export default function ComposerOptions({
  model = null,
  models,
  selected = null,
  onSelect,
  collapsed = false,
  children,
}: ComposerOptionsProps) {
  const [prefs, setPrefs] = useState<ComposerPrefs>(() => loadComposerPrefs());
  const [openId, setOpenId] = useState<OpenId>(null);
  const [highlight, setHighlight] = useState(0);
  const modelOptions = models ?? [];

  const toolsRef = useRef<HTMLDivElement>(null);
  const modelRef = useRef<HTMLButtonElement>(null);
  const agentRef = useRef<HTMLButtonElement>(null);
  const entropyRef = useRef<HTMLButtonElement>(null);
  const permissionRef = useRef<HTMLButtonElement>(null);
  const appliedProjectRef = useRef<string | null>(null);

  useEffect(() => {
    const applyFor = (id: string) => {
      if (appliedProjectRef.current === id) return;
      const project = loadProjects().find((p) => p.id === id);
      if (!project) return;
      appliedProjectRef.current = id;
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

  // Cross-tab sync: another tab toggling bypass writes localStorage, which fires a
  // `storage` event here (cross-tab only). Re-read the stored mode so this tab's
  // trigger matches what App auto-allows from at notification time.
  useEffect(() => {
    const onStorage = (event: StorageEvent) => {
      if (event.key !== null && event.key !== COMPOSER_KEYS.permissionMode) return;
      const mode = loadComposerPrefs().permissionMode;
      setPrefs((prev) => ({ ...prev, permissionMode: mode }));
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, []);

  // Collapsing hides the whole card chrome: close any open popover so it cannot
  // survive invisibly and reappear on expand.
  useEffect(() => {
    if (collapsed) setOpenId(null);
  }, [collapsed]);

  // A persisted preference is display-only, so it may still name a model the kernel no longer
  // serves (older builds offered one): show it only while the served list has no better
  // information. The run itself always uses the picker's entry, never this label.
  const prefsModel =
    modelOptions.length > 0 && !modelOptions.some((entry) => entry.name === prefs.model)
      ? null
      : prefs.model;
  const modelName = selected?.name ?? model ?? prefsModel;
  // Row resolution comes from the SDK: an explicit pick identifies its own row, without one
  // only an unambiguous name can (a name served by several providers cannot say which
  // provider a run would use) — the TUI picker resolves its marker the same way.
  const { index: selectedModelIndex } = resolveModelMarker(modelOptions, selected, modelName);
  // A name no served row carries is still shown (it is what the kernel reports), but the
  // trigger says so instead of implying the run could pick it. Detection is by name: a name
  // two providers serve also yields `index === -1` (ambiguous, not unserved), so the marker
  // index cannot answer this question.
  const unserved = modelName != null && !modelOptions.some((entry) => entry.name === modelName);
  const triggerRefs = {
    model: modelRef,
    agent: agentRef,
    entropy: entropyRef,
    permission: permissionRef,
  };

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
      const entry = modelOptions[index];
      if (!entry) return;
      setPrefs({ ...prefs, model: entry.name });
      saveComposerPrefs({ model: entry.name });
      onSelect?.(entry);
    } else if (id === "agent") {
      const entry = AGENT_LIST[index];
      if (!entry) return;
      setPrefs({ ...prefs, agent: entry.id });
      saveComposerPrefs({ agent: entry.id });
    } else if (id === "entropy") {
      const preset = ENTROPY_PRESETS[index];
      if (!preset) return;
      setPrefs({ ...prefs, entropy: preset.value });
      saveComposerPrefs({ entropy: preset.value });
    } else {
      const entry = PERMISSION_MODES[index];
      if (!entry) return;
      setPrefs({ ...prefs, permissionMode: entry.id });
      saveComposerPrefs({ permissionMode: entry.id });
    }
    closePopover(id);
  };

  const optionCount = (id: Exclude<OpenId, null>): number => {
    if (id === "model") return modelOptions.length;
    if (id === "agent") return AGENT_LIST.length;
    if (id === "entropy") return ENTROPY_PRESETS.length;
    return PERMISSION_MODES.length;
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
          data-served={unserved ? "false" : undefined}
          onClick={() =>
            openPanel("model", selectedModelIndex < 0 ? 0 : selectedModelIndex)
          }
        >
          <span className="g">◆</span>
          {modelName ?? "模型"}
          {unserved && <span className="aj-composer__unserved">当前未提供</span>}
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
          {modelOptions.length === 0 ? (
            <div className="pop-empty" role="presentation">
              {NO_MODEL_HINT}
            </div>
          ) : null}
          {modelOptions.map((entry, index) => {
            const isSelected = index === selectedModelIndex;
            return (
              <div
                key={modelKey(entry)}
                role="option"
                aria-selected={isSelected}
                className={`pop-item${isSelected ? " sel-item" : ""}${highlight === index ? " hl" : ""}`}
                onClick={() => selectOption("model", index)}
              >
                <div>
                  <div className="nm">{entry.name}</div>
                  <div className="cap">{entry.cap}</div>
                </div>
                {isSelected && <span className="tick">✓</span>}
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

      <div className="sel-wrap">
        <button
          type="button"
          id="composer-permission-trigger"
          ref={permissionRef}
          className="sel"
          data-mode={prefs.permissionMode}
          aria-haspopup="listbox"
          aria-expanded={openId === "permission"}
          onClick={() => openPanel("permission", permissionIndex(prefs.permissionMode))}
        >
          <span className="g">⚑</span>
          {PERMISSION_MODES[permissionIndex(prefs.permissionMode)].label}
          <span className="cv">⌄</span>
        </button>
        <OptionPopover
          open={openId === "permission"}
          onClose={() => closePopover("permission")}
          anchorRef={permissionRef}
          className="pop pop-right pop-perm"
          labelledBy="composer-permission-title"
          role="listbox"
          tabIndex={-1}
          onKeyDown={handleKeyDown}
        >
          <div className="pop-title" id="composer-permission-title">权限模式</div>
          {PERMISSION_MODES.map((entry, index) => {
            const selected = prefs.permissionMode === entry.id;
            return (
              <div
                key={entry.id}
                role="option"
                aria-selected={selected}
                className={`pop-item${selected ? " sel-item" : ""}${highlight === index ? " hl" : ""}`}
                onClick={() => selectOption("permission", index)}
              >
                <div>
                  <div className="nm">{entry.label}</div>
                  <div className="cap">{entry.cap}</div>
                </div>
                {selected && <span className="tick">✓</span>}
              </div>
            );
          })}
          <div className="pop-hint">自动允许仅放行内核已确认可审批的请求。</div>
        </OptionPopover>
      </div>

      <span className="spacer" />
      {children}
    </div>
  );
}
