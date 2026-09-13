import type { ComponentProps } from "react";
import { expect, test, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import AskQuestionCard, { type AskQuestionView } from "./AskQuestionCard";

function singleQuestion(): AskQuestionView {
  return {
    questionId: "q1",
    question: "请选择本次重构的范围？",
    options: [
      { id: "ui", label: "仅前端三栏布局" },
      { id: "full", label: "三栏 + 协议扩展", hint: "推荐" },
    ],
    allowMultiple: false,
    allowOther: true,
  };
}

function multiQuestion(): AskQuestionView {
  return {
    questionId: "q2",
    question: "请选择纳入本次交付的能力？",
    options: [
      { id: "a", label: "协议类型" },
      { id: "b", label: "RunPanel 适配" },
      { id: "c", label: "TUI 接线" },
    ],
    allowMultiple: true,
    allowOther: false,
  };
}

function renderCard(
  question: AskQuestionView,
  props: Partial<ComponentProps<typeof AskQuestionCard>> = {},
) {
  return render(
    <AskQuestionCard
      question={question}
      collapsed={false}
      onToggleCollapsed={() => {}}
      onSubmit={() => {}}
      onSkip={() => {}}
      {...props}
    />,
  );
}

test("radio selection replaces the selection and submits one option id", () => {
  const onSubmit = vi.fn();
  renderCard(singleQuestion(), { onSubmit });
  expect(screen.getAllByRole("radio")).toHaveLength(2);
  expect(screen.queryAllByRole("checkbox")).toHaveLength(0);

  const submit = screen.getByRole("button", { name: "提交" });
  expect(submit).toBeDisabled();

  fireEvent.click(screen.getByText("仅前端三栏布局"));
  fireEvent.click(screen.getByText("三栏 + 协议扩展"));
  expect(screen.getByRole("radio", { name: /三栏 \+ 协议扩展/ })).toBeChecked();
  expect(screen.getByRole("radio", { name: /仅前端三栏布局/ })).not.toBeChecked();

  expect(submit).toBeEnabled();
  fireEvent.click(submit);
  expect(onSubmit).toHaveBeenCalledWith({ optionIds: ["full"], other: "" });
});

test("checkbox selections allow multiple option ids", () => {
  const onSubmit = vi.fn();
  renderCard(multiQuestion(), { onSubmit });
  expect(screen.getAllByRole("checkbox")).toHaveLength(3);
  expect(screen.queryAllByRole("radio")).toHaveLength(0);

  const submit = screen.getByRole("button", { name: "提交" });
  expect(submit).toBeDisabled();

  fireEvent.click(screen.getByText("协议类型"));
  fireEvent.click(screen.getByText("RunPanel 适配"));
  expect(submit).toBeEnabled();
  fireEvent.click(submit);
  expect(onSubmit).toHaveBeenCalledWith({ optionIds: ["a", "b"], other: "" });
});

test("other text is carried separately from option ids", () => {
  const onSubmit = vi.fn();
  const { unmount } = renderCard(singleQuestion(), { onSubmit });
  fireEvent.change(screen.getByLabelText("其他回答"), { target: { value: "自定义范围" } });
  fireEvent.click(screen.getByRole("button", { name: "提交" }));
  expect(onSubmit).toHaveBeenCalledWith({ optionIds: [], other: "自定义范围" });
  unmount();

  const onSubmitBoth = vi.fn();
  renderCard(singleQuestion(), { onSubmit: onSubmitBoth });
  fireEvent.click(screen.getByText("三栏 + 协议扩展"));
  fireEvent.change(screen.getByLabelText("其他回答"), { target: { value: "补充说明" } });
  fireEvent.click(screen.getByRole("button", { name: "提交" }));
  expect(onSubmitBoth).toHaveBeenCalledWith({ optionIds: ["full"], other: "补充说明" });
});

test("submit is disabled when nothing is selected and no other text", () => {
  renderCard(multiQuestion());
  expect(screen.getByRole("button", { name: "提交" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "跳过" })).toBeEnabled();
});

test("other input is hidden when allowOther is false", () => {
  renderCard(multiQuestion());
  expect(screen.queryByLabelText("其他回答")).not.toBeInTheDocument();
});

test("collapsed summary joins all selected labels with 、", () => {
  const { rerender } = renderCard(multiQuestion());
  fireEvent.click(screen.getByText("协议类型"));
  fireEvent.click(screen.getByText("RunPanel 适配"));
  fireEvent.click(screen.getByText("TUI 接线"));
  rerender(
    <AskQuestionCard
      question={multiQuestion()}
      collapsed
      onToggleCollapsed={() => {}}
      onSubmit={() => {}}
      onSkip={() => {}}
    />,
  );
  expect(screen.getByText(/已选择：协议类型、RunPanel 适配、TUI 接线/)).toBeInTheDocument();
});

test("collapsed summary reflects internally selected option", () => {
  const { rerender } = renderCard(singleQuestion());
  fireEvent.click(screen.getByText("三栏 + 协议扩展"));
  rerender(
    <AskQuestionCard
      question={singleQuestion()}
      collapsed
      onToggleCollapsed={() => {}}
      onSubmit={() => {}}
      onSkip={() => {}}
    />,
  );
  expect(screen.getByText(/已选择：三栏 \+ 协议扩展/)).toBeInTheDocument();
});

test("collapsed summary falls back to selectedLabel when no internal selection", () => {
  renderCard(singleQuestion(), { collapsed: true, selectedLabel: "仅前端三栏布局" });
  expect(screen.getByText(/已选择：仅前端三栏布局/)).toBeInTheDocument();
});

test("clicking the collapsed summary re-expands via onToggleCollapsed", () => {
  const onToggleCollapsed = vi.fn();
  renderCard(singleQuestion(), { collapsed: true, onToggleCollapsed });
  fireEvent.click(screen.getByRole("button", { name: /已选择/ }));
  expect(onToggleCollapsed).toHaveBeenCalled();
});

test("skip stays enabled and fires onSkip", () => {
  const onSkip = vi.fn();
  renderCard(singleQuestion(), { onSkip });
  fireEvent.click(screen.getByRole("button", { name: "跳过" }));
  expect(onSkip).toHaveBeenCalled();
});
