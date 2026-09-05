export type ModelListInput = {
  default_model: string;
  providers: string[];
};

export function renderModelList(input: ModelListInput): string {
  const { default_model, providers } = input;
  const rows = providers.map((p) => (p === default_model ? `▶ ${p}` : `  ${p}`));
  const hint =
    "model is set server-side in config.toml [model] default_model (restart kernel to change)";
  return [...rows, hint].join("\n");
}

export function ModelSelector(props: ModelListInput): React.ReactNode {
  const out = renderModelList(props);
  const lines = out.split("\n");
  return (
    <box flexDirection="column" border padding={1}>
      {lines.map((line, i) => (
        <text key={i}>{line}</text>
      ))}
    </box>
  );
}
