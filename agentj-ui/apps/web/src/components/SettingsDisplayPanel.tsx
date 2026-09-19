import { useCallback, useEffect, useState } from "react";
import {
  DISPLAY_PREFS_DEFAULTS,
  FONT_SIZE_LIMITS,
  loadDisplayPrefs,
  saveDisplayPrefs,
  subscribeDisplayPrefs,
} from "../displayPrefsStore";
import type {
  DisplayDensity,
  DisplayLineHeight,
  DisplayPrefs,
  FontSizeKey,
} from "../displayPrefsStore";
import "./settingsDialog.css";

type FontGroup = { key: FontSizeKey; label: string };

const FONT_GROUPS: FontGroup[] = [
  { key: "fontConversation", label: "对话/轨迹" },
  { key: "fontLeft", label: "左侧栏" },
  { key: "fontRight", label: "右侧栏" },
  { key: "fontCode", label: "代码" },
];

type LineHeightOption = { label: string; value: DisplayLineHeight };

const LINE_HEIGHT_OPTIONS: LineHeightOption[] = [
  { label: "紧凑", value: "compact" },
  { label: "默认", value: "default" },
  { label: "宽松", value: "relaxed" },
];

type DensityOption = { label: string; value: DisplayDensity };

const DENSITY_OPTIONS: DensityOption[] = [
  { label: "默认", value: "default" },
  { label: "紧凑", value: "compact" },
];

function clamp(n: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, n));
}

type FontSizeRowProps = {
  label: string;
  value: number;
  min: number;
  max: number;
  onChange: (value: number) => void;
};

function FontSizeRow({ label, value, min, max, onChange }: FontSizeRowProps) {
  const [draft, setDraft] = useState(String(value));

  useEffect(() => {
    setDraft(String(value));
  }, [value]);

  const commit = () => {
    const parsed = Number(draft);
    if (draft.trim() === "" || !Number.isFinite(parsed)) {
      setDraft(String(value));
      return;
    }
    const next = clamp(Math.round(parsed), min, max);
    setDraft(String(next));
    if (next !== value) onChange(next);
  };

  return (
    <div className="aj-settings-font-row">
      <span className="aj-settings-font-row__label">{label}</span>
      <input
        type="range"
        className="aj-settings-range"
        aria-label={`${label}字号`}
        min={min}
        max={max}
        step={1}
        value={value}
        onChange={(event) => onChange(Number(event.target.value))}
      />
      <span className="aj-settings-font-row__value">
        <input
          type="number"
          className="aj-settings-number"
          aria-label={`${label}字号数值`}
          min={min}
          max={max}
          step={1}
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onBlur={commit}
          onKeyDown={(event) => {
            if (event.key === "Enter") commit();
          }}
        />
        <span className="aj-settings-font-unit">px</span>
      </span>
    </div>
  );
}

export default function SettingsDisplayPanel() {
  const [prefs, setPrefs] = useState(loadDisplayPrefs);

  useEffect(() => subscribeDisplayPrefs(setPrefs), []);

  const apply = useCallback((patch: Partial<DisplayPrefs>) => {
    setPrefs(saveDisplayPrefs(patch));
  }, []);

  const reset = useCallback(() => {
    setPrefs(saveDisplayPrefs({ ...DISPLAY_PREFS_DEFAULTS }));
  }, []);

  return (
    <div className="aj-settings-group">
      <fieldset className="aj-settings-fieldset">
        <legend className="aj-settings-legend">字体大小（像素）</legend>
        {FONT_GROUPS.map((group) => (
          <FontSizeRow
            key={group.key}
            label={group.label}
            value={prefs[group.key]}
            min={FONT_SIZE_LIMITS[group.key].min}
            max={FONT_SIZE_LIMITS[group.key].max}
            onChange={(value) => apply({ [group.key]: value })}
          />
        ))}
      </fieldset>
      <fieldset className="aj-settings-fieldset">
        <legend className="aj-settings-legend">行高</legend>
        <div className="aj-settings-options">
          {LINE_HEIGHT_OPTIONS.map((option) => (
            <label key={option.value} className="aj-settings-radio">
              <input
                type="radio"
                name="aj-display-line-height"
                checked={prefs.lineHeight === option.value}
                onChange={() => apply({ lineHeight: option.value })}
              />
              <span>{option.label}</span>
            </label>
          ))}
        </div>
      </fieldset>
      <fieldset className="aj-settings-fieldset">
        <legend className="aj-settings-legend">界面密度</legend>
        <div className="aj-settings-options">
          {DENSITY_OPTIONS.map((option) => (
            <label key={option.value} className="aj-settings-radio">
              <input
                type="radio"
                name="aj-display-density"
                checked={prefs.density === option.value}
                onChange={() => apply({ density: option.value })}
              />
              <span>{option.label}</span>
            </label>
          ))}
        </div>
      </fieldset>
      <div className="aj-settings-toggle">
        <span>减弱动画</span>
        <button
          type="button"
          role="switch"
          aria-label="减弱动画"
          aria-checked={prefs.reducedMotion}
          className="aj-settings-switch"
          onClick={() => apply({ reducedMotion: !prefs.reducedMotion })}
        >
          {prefs.reducedMotion ? "已开启" : "已关闭"}
        </button>
      </div>
      <div className="aj-settings-foot">
        <button type="button" className="aj-button--ghost" onClick={reset}>
          恢复默认
        </button>
      </div>
    </div>
  );
}
