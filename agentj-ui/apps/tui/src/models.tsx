import { useEffect, useState } from "react";
import { useKeyboard } from "@opentui/react";
import { METHODS } from "@agentj/sdk";
import type { RpcClient } from "@agentj/sdk";

export type ModelEntry = {
  name: string;
  provider?: string;
};

export type ModelListInput = {
  default_model: string;
  providers: ModelEntry[];
};

export const MODEL_CONFIG: ModelListInput = {
  default_model: "deepseek-chat",
  providers: [{ name: "deepseek-chat" }, { name: "opencode-go/kimi-k3" }],
};

const MODEL_LIST_HINT = "model 运行期可切换 · Enter 选择 · j/k 移动";

function staticEntries(): ModelEntry[] {
  return MODEL_CONFIG.providers.map(({ name }) => ({ name }));
}

export function providersFromStatus(data: Record<string, unknown>): ModelEntry[] {
  const prov = data?.["providers"];
  if (!Array.isArray(prov) || prov.length === 0) return staticEntries();
  const entries = (prov as Array<{ id?: unknown; models?: unknown }>).flatMap((p) => {
    const provider = typeof p?.id === "string" ? p.id : undefined;
    if (!Array.isArray(p?.models)) return [];
    return p.models
      .filter((m): m is string => typeof m === "string")
      .map((name) => (provider ? { name, provider } : { name }));
  });
  return entries.length > 0 ? entries : staticEntries();
}

export function useModelConfig(client: RpcClient | null): ModelListInput {
  const [cfg, setCfg] = useState<ModelListInput>(MODEL_CONFIG);
  useEffect(() => {
    if (!client) return;
    client
      .request(METHODS.kernelStatus)
      .then((r) => {
        const data = r as unknown as Record<string, unknown>;
        const dm = data?.["default_model"];
        if (typeof dm === "string" && dm.length > 0) {
          const providers = providersFromStatus(data);
          setCfg({ default_model: dm, providers });
        }
      })
      .catch(() => {});
  }, [client]);
  return cfg;
}

export function renderModelList(input: ModelListInput & { selected?: ModelEntry | null }): string {
  const { default_model, providers, selected } = input;
  const active = selected?.name ?? default_model;
  const rows = providers.map((e) => (e.name === active ? `▶ ${e.name}` : `  ${e.name}`));
  return [...rows, MODEL_LIST_HINT].join("\n");
}

export type ModelSelectorProps = ModelListInput & {
  selected?: ModelEntry | null;
  onSelect?: (entry: ModelEntry) => void;
};

function highlightIndex(providers: ModelEntry[], anchor: string): number {
  const i = providers.findIndex((e) => e.name === anchor);
  return i >= 0 ? i : 0;
}

export function ModelSelector(props: ModelSelectorProps): React.ReactNode {
  const { default_model, providers, selected, onSelect } = props;
  const anchor = selected?.name ?? default_model;
  const [idx, setIdx] = useState(() => highlightIndex(providers, anchor));
  useEffect(() => {
    setIdx(highlightIndex(providers, anchor));
  }, [providers, anchor]);
  useKeyboard((key) => {
    const name = (key.name ?? "").toLowerCase();
    if (name === "j" || name === "down") {
      setIdx((v) => Math.min(providers.length - 1, v + 1));
      return;
    }
    if (name === "k" || name === "up") {
      setIdx((v) => Math.max(0, v - 1));
      return;
    }
    const isEnter = name === "return" || name === "enter" || key.sequence === "\r";
    const entry = providers[idx];
    if (isEnter && entry) onSelect?.(entry);
  });
  const highlighted = providers[idx] ?? selected ?? null;
  const lines = renderModelList({ default_model, providers, selected: highlighted }).split("\n");
  return (
    <box flexDirection="column" border padding={1}>
      {lines.map((line, i) => (
        <text key={i}>{line}</text>
      ))}
    </box>
  );
}
