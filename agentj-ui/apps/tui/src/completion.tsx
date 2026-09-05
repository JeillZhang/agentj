export function completePath(prefix: string, files: string[]): string[] {
  const q = prefix.startsWith("@") ? prefix.slice(1) : prefix;
  if (q === "") return [...files];
  return files.filter((f) => f.startsWith(q));
}

export function getAtPrefix(text: string): string | null {
  const m = text.match(/@\S*$/);
  return m ? m[0] : null;
}

export function insertCompletion(text: string, chosen: string): string {
  const idx = text.lastIndexOf("@");
  if (idx === -1) return text + chosen;
  const after = text.slice(idx);
  const spaceIdx = after.search(/\s/);
  if (spaceIdx === -1) return text.slice(0, idx) + chosen;
  return text.slice(0, idx) + chosen + text.slice(idx + spaceIdx);
}

export type CompletionProps = {
  matches: string[];
  selected: number;
  onSelect: (file: string) => void;
};

export function CompletionOverlay(props: CompletionProps): React.ReactNode {
  const { matches, selected } = props;
  if (matches.length === 0) return null;
  return (
    <box flexDirection="column" border padding={1}>
      {matches.map((m, i) => (
        <text key={m}>{i === selected ? `▶ ${m}` : `  ${m}`}</text>
      ))}
    </box>
  );
}
