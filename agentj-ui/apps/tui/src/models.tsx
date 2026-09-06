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

export function useModelConfig(client: RpcClient | null): ModelListInput {
  const [cfg, setCfg] = useState<ModelListInput>(MODEL_CONFIG);
  useEffect(() => {
    if (!client) return;
    client
      .request(METHODS.kernelStatus)
      .then((r) => {
        const data = r as unknown as Record<string, unknown>;
        const dm = data?.["default_model"];
        const prov = data?.["providers"];
        if (typeof dm === "string" && dm.length > 0) {
          const providers = Array.isArray(prov) && prov.length > 0 ? (prov as string[]) : MODEL_CONFIG.providers;
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
