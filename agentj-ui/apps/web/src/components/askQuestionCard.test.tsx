import { expect, test, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import AskQuestionCard from "./AskQuestionCard";

const q = {
  id: "q1",
  question: "请选择本次重构的范围？",
  options: [
    { id: "ui", label: "仅前端三栏布局" },
    { id: "full", label: "三栏 + 协议扩展", hint: "推荐" },
  ],
};

test("select and submit", () => {
  const onSubmit = vi.fn();
  render(
    <AskQuestionCard
      question={q}
      collapsed={false}
      onToggleCollapsed={() => {}}
      onSubmit={onSubmit}
      onSkip={() => {}}
    />,
  );
  const submit = screen.getByRole("button", { name: "提交" });
  expect(submit).toBeDisabled();
  fireEvent.click(screen.getByText("三栏 + 协议扩展"));
  expect(submit).toBeEnabled();
  fireEvent.click(submit);
  expect(onSubmit).toHaveBeenCalledWith({ optionId: "full", other: "" });
});

test("other input overrides", () => {
  const onSubmit = vi.fn();
  render(
    <AskQuestionCard
      question={q}
      collapsed={false}
      onToggleCollapsed={() => {}}
      onSubmit={onSubmit}
      onSkip={() => {}}
    />,
  );
  fireEvent.change(screen.getByLabelText("其他回答"), { target: { value: "自定义范围" } });
  fireEvent.click(screen.getByRole("button", { name: "提交" }));
  expect(onSubmit).toHaveBeenCalledWith({ optionId: null, other: "自定义范围" });
});

test("skip and collapsed summary", () => {
  const onSkip = vi.fn();
  const { rerender } = render(
    <AskQuestionCard
      question={q}
      collapsed={false}
      onToggleCollapsed={() => {}}
      onSubmit={() => {}}
      onSkip={onSkip}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "跳过" }));
  expect(onSkip).toHaveBeenCalled();
  rerender(
    <AskQuestionCard
      question={q}
      collapsed
      selectedLabel="三栏 + 协议扩展"
      onToggleCollapsed={() => {}}
      onSubmit={() => {}}
      onSkip={() => {}}
    />,
  );
  expect(screen.getByText(/已选择：三栏 \+ 协议扩展/)).toBeInTheDocument();
});
