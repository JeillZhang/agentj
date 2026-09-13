import { useState } from "react";
import "./ask.css";

export type AskOptionView = {
  id: string;
  label: string;
  hint?: string;
};

export type AskQuestionView = {
  questionId: string;
  question: string;
  header?: string;
  options: AskOptionView[];
  allowMultiple: boolean;
  allowOther: boolean;
};

export type AskAnswer = {
  optionIds: string[];
  other: string;
};

export type AskQuestionCardProps = {
  question: AskQuestionView;
  collapsed: boolean;
  selectedLabel?: string;
  onToggleCollapsed: () => void;
  onSubmit: (answer: AskAnswer) => void;
  onSkip: () => void;
};

/** Composer question card (T20/T34): protocol-shaped question view, collapsed one-line summary. */
export default function AskQuestionCard({
  question,
  collapsed,
  selectedLabel,
  onToggleCollapsed,
  onSubmit,
  onSkip,
}: AskQuestionCardProps) {
  const [selectedIds, setSelectedIds] = useState<string[]>([]);
  const [other, setOther] = useState("");
  const hasOther = other.trim().length > 0;
  const canSubmit = selectedIds.length > 0 || hasOther;
  const qid = question.questionId;

  if (collapsed) {
    const labels = selectedIds
      .map((id) => question.options.find((o) => o.id === id)?.label)
      .filter((label): label is string => label !== undefined);
    const summary =
      labels.length > 0 ? labels.join("、") : selectedLabel ?? (hasOther ? other : "—");
    return (
      <section className="ask" aria-label={`模型提问：${question.question}`}>
        <button type="button" className="ask-collapsed" onClick={onToggleCollapsed}>
          {`✓ 已选择：${summary} ⌄`}
        </button>
      </section>
    );
  }

  function toggleOption(id: string) {
    setSelectedIds((prev) => {
      if (!question.allowMultiple) return [id];
      return prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id];
    });
  }

  const handleSubmit = () => {
    if (!canSubmit) return;
    onSubmit({ optionIds: selectedIds, other: hasOther ? other : "" });
  };

  const inputType = question.allowMultiple ? "checkbox" : "radio";

  return (
    <section className="ask" role="group" aria-labelledby={`ask-${qid}`}>
      <div className="ask-head">
        <span className="ask-badge" aria-hidden="true">?</span>
        <h4 className="ask-q" id={`ask-${qid}`}>{question.question}</h4>
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
      <p className="ask-cap">{question.header ?? "模型需要你确认后再继续。"}</p>
      <div className="ask-opts">
        {question.options.map((option) => {
          const on = selectedIds.includes(option.id);
          return (
            <label key={option.id} className={`opt ${on ? "on" : ""}`}>
              <input
                type={inputType}
                name={`ask-${qid}`}
                value={option.id}
                checked={on}
                onChange={() => toggleOption(option.id)}
                style={question.allowMultiple ? { accentColor: "var(--aj-accent)" } : undefined}
              />
              <span className="lb">{option.label}</span>
              {option.hint && <span className="hnt">{option.hint}</span>}
            </label>
          );
        })}
      </div>
      <div className="ask-foot">
        {question.allowOther && (
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
        )}
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
