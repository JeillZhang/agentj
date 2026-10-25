import { useEffect, useState } from "react";
import { useKeyboard } from "@opentui/react";
import { METHODS, formatContextWindow, resolveModelMarker } from "@agentj/sdk";
import type { RpcClient } from "@agentj/sdk";

export type ModelEntry = {
  name: string;
  provider?: string;
  cap?: string;
};

/** One `model/list` row, narrowed to the field the picker labels rows with. */
type ModelWindow = { id: string; context_window?: number | null };

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

/** Kernel-reported models, or null when the kernel reports no model at all. */
function kernelModelEntries(data: Record<string, unknown>): ModelEntry[] | null {
  const prov = data?.["providers"];
  if (!Array.isArray(prov)) return null;
  const entries = (prov as Array<{ id?: unknown; models?: unknown }>).flatMap((p) => {
    const provider = typeof p?.id === "string" ? p.id : undefined;
    if (!Array.isArray(p?.models)) return [];
    return p.models
      .filter((m): m is string => typeof m === "string")
      .map((name) => (provider ? { name, provider } : { name }));
  });
  return entries.length > 0 ? entries : null;
}

export function providersFromStatus(data: Record<string, unknown>): ModelEntry[] {
  return kernelModelEntries(data) ?? staticEntries();
}

/**
 * Labels entries with the catalog context window. A model the catalog omits (or
 * reports no window for) keeps its entry object untouched, so its row is unchanged.
 */
export function withContextWindows(entries: ModelEntry[], models: ModelWindow[]): ModelEntry[] {
  const windows = new Map<string, number>();
  for (const model of models) {
    if (typeof model.context_window === "number") windows.set(model.id, model.context_window);
  }
  return entries.map((e) => {
    const label = formatContextWindow(windows.get(e.name) ?? null);
    return label ? { ...e, cap: label } : e;
  });
}

/**
 * Catalog windows, fetched once per provider.
 *
 * `model/list` is provider-scoped — a param-less call answers for the configured default
 * provider only — so a served row of any other provider would stay unlabelled. Ids are
 * provider-scoped too (`opencode-go` namespaces its catalog as `opencode-go/<model>`), so the
 * legs merge into one flat id → window list and a row is labelled only by an id its own
 * provider published. A failing leg contributes nothing, dropping that provider's labels only.
 */
async function loadContextWindows(client: RpcClient, entries: ModelEntry[]): Promise<ModelWindow[]> {
  const providers = [
    ...new Set(entries.map((e) => e.provider).filter((p): p is string => Boolean(p))),
  ];
  const catalogs = await Promise.all(
    providers.map((provider) =>
      client
        .request(METHODS.modelList, { provider })
        .then((list) => (list as { models?: ModelWindow[] }).models ?? [])
        .catch(() => [] as ModelWindow[]),
    ),
  );
  return catalogs.flat();
}

/**
 * Kernel model config, re-read whenever `refreshSeq` changes: a credential change makes the
 * served catalog different (models appear or disappear), so the caller bumps the counter
 * instead of remounting the app.
 */
export function useModelConfig(client: RpcClient | null, refreshSeq = 0): ModelListInput {
  const [cfg, setCfg] = useState<ModelListInput>(MODEL_CONFIG);
  useEffect(() => {
    if (!client) return;
    client
      .request(METHODS.kernelStatus)
      .then((r) => {
        const data = r as unknown as Record<string, unknown>;
        // The kernel's model list is authoritative on its own: a missing/empty
        // default_model only means "no default row", not "use the static list"
        // (a static entry would submit without a provider tag and mis-route).
        const providers = kernelModelEntries(data);
        if (!providers) return;
        const dm = data?.["default_model"];
        // The context window is a catalog fact (model/list), not a kernel/status one, so
        // it rides second requests, one per provider.
        return loadContextWindows(client, providers).then((models) =>
          setCfg({
            default_model: typeof dm === "string" ? dm : "",
            providers: withContextWindows(providers, models),
          }),
        );
      })
      .catch(() => {});
  }, [client, refreshSeq]);
  return cfg;
}

/**
 * Index of the row carrying the ▶ marker, or -1 when no row is authoritative:
 * an explicit pick identifies its own row, a bare default only an unambiguous name.
 * The rule lives in the SDK so the Web picker marks rows the same way.
 */
function markerIndex(
  providers: ModelEntry[],
  selected: ModelEntry | null | undefined,
  defaultModel: string,
): number {
  return resolveModelMarker(providers, selected, defaultModel).index;
}

export function renderModelList(input: ModelListInput & { selected?: ModelEntry | null }): string {
  const marker = markerIndex(input.providers, input.selected, input.default_model);
  const rows = input.providers.map(
    (e, i) => (i === marker ? `▶ ${e.name}` : `  ${e.name}`) + (e.cap ? `  ${e.cap}` : ""),
  );
  return [...rows, MODEL_LIST_HINT].join("\n");
}

export type ModelSelectorProps = ModelListInput & {
  selected?: ModelEntry | null;
  onSelect?: (entry: ModelEntry) => void;
};

/** -1 means "no authoritative row"; the cursor then sits on the first row. */
function cursorOf(idx: number): number {
  return idx < 0 ? 0 : idx;
}

export function ModelSelector(props: ModelSelectorProps): React.ReactNode {
  const { default_model, providers, selected, onSelect } = props;
  const anchor = markerIndex(providers, selected, default_model);
  const [idx, setIdx] = useState(() => anchor);
  useEffect(() => {
    setIdx(markerIndex(providers, selected, default_model));
  }, [providers, selected, default_model]);
  useKeyboard((key) => {
    const name = (key.name ?? "").toLowerCase();
    if (name === "j" || name === "down") {
      setIdx((v) => Math.min(providers.length - 1, cursorOf(v) + 1));
      return;
    }
    if (name === "k" || name === "up") {
      setIdx((v) => Math.max(0, cursorOf(v) - 1));
      return;
    }
    const isEnter = name === "return" || name === "enter" || key.sequence === "\r";
    const entry = providers[cursorOf(idx)];
    if (isEnter && entry) onSelect?.(entry);
  });
  // The cursor row is the marker once it is authoritative (idx >= 0); an -1 anchor
  // falls back to the raw pick so an unresolvable one still marks no row.
  const marked = idx >= 0 ? providers[idx] : selected ?? null;
  const lines = renderModelList({ default_model, providers, selected: marked }).split("\n");
  return (
    <box flexDirection="column" border padding={1}>
      {lines.map((line, i) => (
        <text key={i}>{line}</text>
      ))}
    </box>
  );
}
