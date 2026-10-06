import { useEffect, useState } from "react";
import { METHODS } from "@agentj/sdk";
import type { RpcClient } from "@agentj/sdk";

export type ModelListInput = {
  default_model: string;
  providers: string[];
};

export const MODEL_CONFIG: ModelListInput = {
  default_model: "deepseek-chat",
  providers: ["deepseek-chat", "opencode-go/kimi-k3"],
};

export function providersFromStatus(data: Record<string, unknown>): string[] {
  const prov = data?.["providers"];
  if (!Array.isArray(prov) || prov.length === 0) return MODEL_CONFIG.providers;
  const ids = (prov as Array<{ models?: unknown }>).flatMap((p) =>
    Array.isArray(p?.models)
      ? p.models.filter((m): m is string => typeof m === "string")
      : [],
  );
  return ids.length > 0 ? ids : MODEL_CONFIG.providers;
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
