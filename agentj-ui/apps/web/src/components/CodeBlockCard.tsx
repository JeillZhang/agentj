// agentj-ui/apps/web/src/components/CodeBlockCard.tsx
// react-markdown `pre` override: a code card with a language label, a collapse
// toggle and clipboard copy feedback. Collapsed state lives inside the component,
// so it survives streaming re-renders that only append text to the children.
import { isValidElement, useEffect, useRef, useState, type ReactNode } from "react";
import { Check, ChevronDown, ChevronRight, Copy } from "lucide-react";
import { copyText } from "../copyText";
import "./codeblock.css";

type Props = { children?: ReactNode };
type Status = "idle" | "copied" | "error";

// Walk the (possibly array) children until a valid element exposes a
// `language-*` className (react-markdown puts it on the inner <code>).
function languageOf(children: ReactNode): string | null {
  const nodes = Array.isArray(children) ? children : [children];
  for (const node of nodes) {
    if (!isValidElement<{ className?: string }>(node)) continue;
    const match = node.props.className?.match(/language-([\w-]+)/);
    if (match) return match[1];
  }
  return null;
}

export default function CodeBlockCard({ children }: Props) {
  const [collapsed, setCollapsed] = useState(false);
  const [status, setStatus] = useState<Status>("idle");
  const preRef = useRef<HTMLPreElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(
    () => () => {
      if (timer.current) clearTimeout(timer.current);
    },
    [],
  );

  const onCopy = async () => {
    const ok = await copyText(preRef.current?.textContent ?? "");
    setStatus(ok ? "copied" : "error");
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => setStatus("idle"), 1500);
  };

  return (
    <figure className="aj-code">
      <figcaption className="aj-code__head">
        <button
          type="button"
          className="aj-code__toggle"
          aria-expanded={!collapsed}
          aria-label={collapsed ? "展开代码" : "折叠代码"}
          onClick={() => setCollapsed((c) => !c)}
        >
          {collapsed ? <ChevronRight size={13} /> : <ChevronDown size={13} />}
          <span className="aj-code__lang">{languageOf(children) ?? "code"}</span>
        </button>
        <span className="aj-code__actions">
          <span className="aj-code__status" role="status" aria-live="polite">
            {status === "copied" ? "已复制" : status === "error" ? "复制失败" : ""}
          </span>
          <button type="button" className="aj-code__copy" aria-label="复制代码" onClick={onCopy}>
            {status === "copied" ? <Check size={13} /> : <Copy size={13} />}
          </button>
        </span>
      </figcaption>
      <pre ref={preRef} className="aj-code__body" hidden={collapsed}>
        {children}
      </pre>
    </figure>
  );
}
