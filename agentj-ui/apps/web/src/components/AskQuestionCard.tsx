import { useState } from "react";
import type { DemoQuestion } from "../demoData";
import "./ask.css";

export type AskAnswer = {
  optionId: string | null;
  other: string;
};

export type AskQuestionCardProps = {
  question: DemoQuestion;
  collapsed: boolean;
  selectedLabel?: string;
  onToggleCollapsed: () => void;
  onSubmit: (answer: AskAnswer) => void;
  onSkip: () => void;
};

/** Composer question card (T20): expanded option picker or one-line collapsed summary. */
export default function AskQuestionCard({
  question,
  collapsed,
  selectedLabel,
  onToggleCollapsed,
  onSubmit,
  onSkip,
}: AskQuestionCardProps) {
  const [selected, setSelected] = useState<string | null>(null);
  const [other, setOther] = useState("");
  const hasOther = other.trim().length > 0;
  const canSubmit = selected !== null || hasOther;

  if (collapsed) {
    const chosenLabel =
      selected !== null
        ? question.options.find((o) => o.id === selected)?.label
        : undefined;
    const summary = chosenLabel ?? selectedLabel ?? (hasOther ? other : "—");
    return (
      <section className="ask" aria-label={`模型提问：${question.question}`}>
        <button type="button" className="ask-collapsed" onClick={onToggleCollapsed}>
          {`✓ 已选择：${summary} ⌄`}
        </button>
      </section>
    );
  }

  const handleSubmit = () => {
    if (!canSubmit) return;
    onSubmit(hasOther ? { optionId: null, other } : { optionId: selected, other: "" });
  };

  return (
    <section className="ask" role="group" aria-labelledby={`ask-${question.id}`}>
      <div className="ask-head">
        <span className="ask-badge" aria-hidden="true">?</span>
        <h4 className="ask-q" id={`ask-${question.id}`}>{question.question}</h4>
        <button
          type="button"
          className="ask-col"
          aria-expanded={true}
          aria-label="折叠提问"
          onClick={onToggleCollapsed}
        >
          ⌄
        </button>
      </div>
      <p className="ask-cap">模型需要你确认后再继续。</p>
      <div className="ask-opts">
        {question.options.map((option) => {
          const on = selected === option.id;
          return (
            <label
              key={option.id}
              className={`opt ${on ? "on" : ""}`}
              onClick={() => setSelected(option.id)}
            >
              <input
                type="radio"
                name={`ask-${question.id}`}
                value={option.id}
                checked={on}
                onChange={() => setSelected(option.id)}
              />
              <span className="lb">{option.label}</span>
              {option.hint && <span className="hnt">{option.hint}</span>}
            </label>
          );
        })}
      </div>
      <div className="ask-foot">
        <input
          className="ask-other"
          type="text"
          aria-label="其他回答"
          placeholder="其他…"
          value={other}
          onChange={(event) => setOther(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") handleSubmit();
          }}
        />
        <button type="button" className="btn-ghost" onClick={onSkip}>
          跳过
        </button>
        <button type="button" className="btn-submit" disabled={!canSubmit} onClick={handleSubmit}>
          提交
        </button>
      </div>
    </section>
  );
}
