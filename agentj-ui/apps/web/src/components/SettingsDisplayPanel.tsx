import { useCallback, useEffect, useState } from "react";
import {
  DISPLAY_PREFS_DEFAULTS,
  loadDisplayPrefs,
  saveDisplayPrefs,
  subscribeDisplayPrefs,
} from "../displayPrefsStore";
import type {
  DisplayDensity,
  DisplayLineHeight,
  DisplayPrefs,
  FontScale,
} from "../displayPrefsStore";
import "./settingsDialog.css";

type FontPrefKey = "fontConversation" | "fontLeft" | "fontRight" | "fontCode";

type FontGroup = { key: FontPrefKey; label: string; name: string };

const FONT_GROUPS: FontGroup[] = [
  { key: "fontConversation", label: "对话/轨迹", name: "aj-display-font-conversation" },
  { key: "fontLeft", label: "左侧栏", name: "aj-display-font-left" },
  { key: "fontRight", label: "右侧栏", name: "aj-display-font-right" },
  { key: "fontCode", label: "代码", name: "aj-display-font-code" },
];

type ScaleOption = { label: string; value: FontScale };

const SCALE_OPTIONS: ScaleOption[] = [
  { label: "小", value: "sm" },
  { label: "中", value: "md" },
  { label: "大", value: "lg" },
  { label: "特大", value: "xl" },
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

/** 显示 settings panel — changes apply immediately (persisted via displayPrefsStore). */
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
      {FONT_GROUPS.map((group) => (
        <fieldset key={group.key} className="aj-settings-fieldset">
          <legend className="aj-settings-legend">{group.label}</legend>
          {SCALE_OPTIONS.map((option) => (
            <label key={option.value} className="aj-settings-radio">
              <input
                type="radio"
                name={group.name}
                checked={prefs[group.key] === option.value}
                onChange={() => apply({ [group.key]: option.value })}
              />
              <span>{option.label}</span>
            </label>
          ))}
        </fieldset>
      ))}
      <fieldset className="aj-settings-fieldset">
        <legend className="aj-settings-legend">行高</legend>
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
      </fieldset>
      <fieldset className="aj-settings-fieldset">
        <legend className="aj-settings-legend">界面密度</legend>
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
