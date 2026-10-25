/** Minimal structural shape needed to flatten a loaded tree for navigation. */
export type NavEntry = {
  path: string;
  kind: "file" | "dir";
};

const ROOT = "";

/**
 * Flattens the loaded `entries` map into the ordered list of visible row paths:
 * depth-first, each directory emitted before its children, and children only
 * walked while the directory is in `expanded`. Collapsed dirs contribute their
 * own row only.
 */
export function visiblePaths(
  entries: ReadonlyMap<string, readonly NavEntry[]>,
  expanded: ReadonlySet<string>,
): string[] {
  const paths: string[] = [];
  const walk = (parent: string): void => {
    for (const node of entries.get(parent) ?? []) {
      paths.push(node.path);
      if (node.kind === "dir" && expanded.has(node.path)) walk(node.path);
    }
  };
  walk(ROOT);
  return paths;
}
