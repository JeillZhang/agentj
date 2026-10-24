// agentj-ui/packages/sdk/src/toolSummary.ts
// Shared one-line summary of a tool call, consumed by the Web run panel. `args` is
// deliberately loose: wire arguments are untyped JSON and may be null/scalar.
export function toolSummary(input: { name: string; args: unknown }): string {
  const args = input.args;
  if (typeof args !== "object" || args === null) return "";
  const a = args as Record<string, unknown>;
  const str = (key: string) => (typeof a[key] === "string" ? (a[key] as string) : "");
  switch (input.name) {
    case "bash":
      return str("command");
    case "read":
    case "write":
      return str("path");
    case "glob":
      return str("pattern");
    case "grep": {
      const pattern = str("pattern");
      const include = str("include");
      return pattern && include && include !== "*" ? `${pattern} · ${include}` : pattern;
    }
    case "todo":
      return Array.isArray(a.items) ? `${a.items.length} 项` : "";
    default:
      return "";
  }
}
