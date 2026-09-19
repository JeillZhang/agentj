// Pure extension → icon mapping for the file tree. No React, no side effects:
// `fileIconFor` only lowercases the extension and looks it up in a static table,
// so it is trivial to unit-test and safe to call during render.
import {
  File,
  FileArchive,
  FileCode2,
  FileCog,
  FileJson,
  FileText,
  Image,
  Palette,
  SquareTerminal,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";

/** Coarse buckets used for the per-category tint modifiers in `filetree.css`. */
export type FileCategory =
  | "code"
  | "json"
  | "doc"
  | "style"
  | "config"
  | "shell"
  | "image"
  | "archive"
  | "default";

export type FileIconMatch = {
  Icon: LucideIcon;
  category: FileCategory;
};

type CategorySpec = {
  category: FileCategory;
  Icon: LucideIcon;
  /** Lowercase extensions, without the leading dot. */
  extensions: readonly string[];
};

const CATEGORIES: readonly CategorySpec[] = [
  {
    category: "code",
    Icon: FileCode2,
    extensions: ["ts", "tsx", "js", "jsx", "mjs", "cjs", "vue", "svelte"],
  },
  { category: "json", Icon: FileJson, extensions: ["json", "jsonc"] },
  { category: "doc", Icon: FileText, extensions: ["md", "mdx", "txt", "rst"] },
  { category: "style", Icon: Palette, extensions: ["css", "scss", "less"] },
  { category: "config", Icon: FileCog, extensions: ["toml", "yaml", "yml", "ini", "env"] },
  { category: "shell", Icon: SquareTerminal, extensions: ["sh", "bash", "zsh"] },
  { category: "image", Icon: Image, extensions: ["png", "jpg", "jpeg", "gif", "svg", "webp", "ico"] },
  { category: "archive", Icon: FileArchive, extensions: ["zip", "gz", "tar", "lock"] },
];

/**
 * Lowercased text after the last dot, or "" when there is none.
 * A leading-dot name (`.env`) yields its stem so dotfiles still classify.
 */
export function extensionOf(name: string): string {
  const dot = name.lastIndexOf(".");
  if (dot < 0) return "";
  return name.slice(dot + 1).toLowerCase();
}

/** Resolves a file name to its lucide icon + category; unknown extensions fall back to `File`. */
export function fileIconFor(name: string): FileIconMatch {
  const ext = extensionOf(name);
  for (const spec of CATEGORIES) {
    if (spec.extensions.includes(ext)) return { Icon: spec.Icon, category: spec.category };
  }
  return { Icon: File, category: "default" };
}
