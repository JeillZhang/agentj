import { useEffect, useState } from "react";
import { useKeyboard } from "@opentui/react";

export const COMMANDS = ["models", "sessions", "themes", "help", "exit", "subscription"] as const;

export function filterCommands(query: string, commands: string[]): string[] {
  const q = query.replace(/^\//, "").toLowerCase().trim();
  if (q === "") return [...commands];
  return commands.filter((c) => c.toLowerCase().startsWith(q));
}

export type PaletteProps = {
  query: string;
  onSelect: (cmd: string) => void;
  onClose: () => void;
};

export function Palette(props: PaletteProps): React.ReactNode {
  const { query, onSelect, onClose } = props;
  const filtered = filterCommands(query, [...COMMANDS]);
  const [idx, setIdx] = useState(0);
  useEffect(() => {
    setIdx(0);
  }, [query]);
  useKeyboard((key) => {
    const name = (key.name ?? "").toLowerCase();
    if (name === "escape") {
      onClose();
      return;
    }
    if (name === "up") {
      setIdx((v) => Math.max(0, v - 1));
      return;
    }
    if (name === "down") {
      setIdx((v) => Math.min(filtered.length - 1, v + 1));
      return;
    }
    const isEnter = name === "return" || name === "enter" || key.sequence === "\r";
    if (isEnter) {
      const cmd = filtered[idx];
      if (cmd) onSelect(cmd);
    }
  });
  if (filtered.length === 0) {
    return (
      <box border padding={1}>
        <text>no commands</text>
      </box>
    );
  }
  return (
    <box flexDirection="column" border padding={1}>
      {filtered.map((c, i) => (
        <text key={c}>{i === idx ? `▶ ${c}` : `  ${c}`}</text>
      ))}
    </box>
  );
}
