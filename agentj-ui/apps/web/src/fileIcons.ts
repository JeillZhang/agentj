// Pure extension → icon mapping for the file tree. No React, no side effects:
// `fileIconFor` only lowercases the extension and looks it up in a static table,
// so it is trivial to unit-test and safe to call during render.
import {
  Database,
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
  | "db"
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
    extensions: [
      "ts",
      "tsx",
      "js",
      "jsx",
      "mjs",
      "cjs",
      "vue",
      "svelte",
      "rs",
      "go",
      "py",
      "rb",
      "java",
      "kt",
      "kts",
      "scala",
      "swift",
      "c",
      "h",
      "cpp",
      "hpp",
      "cc",
      "cxx",
      "hh",
      "cs",
      "php",
      "lua",
      "dart",
      "r",
      "pl",
      "pm",
      "ex",
      "exs",
      "erl",
      "hrl",
      "hs",
      "clj",
      "cljs",
      "cljc",
      "ml",
      "fs",
      "fsx",
      "vb",
      "jl",
      "zig",
      "nim",
      "proto",
      "graphql",
      "gql",
      "html",
      "htm",
      "xml",
    ],
  },
  { category: "json", Icon: FileJson, extensions: ["json", "jsonc"] },
  { category: "doc", Icon: FileText, extensions: ["md", "mdx", "txt", "rst"] },
  { category: "style", Icon: Palette, extensions: ["css", "scss", "less"] },
  { category: "config", Icon: FileCog, extensions: ["toml", "yaml", "yml", "ini", "env"] },
  { category: "shell", Icon: SquareTerminal, extensions: ["sh", "bash", "zsh"] },
  { category: "image", Icon: Image, extensions: ["png", "jpg", "jpeg", "gif", "svg", "webp", "ico"] },
  { category: "archive", Icon: FileArchive, extensions: ["zip", "gz", "tar", "lock"] },
  { category: "db", Icon: Database, extensions: ["sql", "db", "sqlite", "sqlite3"] },
];

/** Extension-less build files matched on their exact basename before the extension scan. */
const FILENAME_SPECIALS: Record<string, FileCategory> = {
  Dockerfile: "code",
  Makefile: "code",
  "CMakeLists.txt": "code",
};

const CATEGORY_ICONS: Record<FileCategory, LucideIcon> = CATEGORIES.reduce(
  (acc, spec) => {
    acc[spec.category] = spec.Icon;
    return acc;
  },
  { default: File } as Record<FileCategory, LucideIcon>,
);

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
  const slash = name.lastIndexOf("/");
  const basename = slash < 0 ? name : name.slice(slash + 1);
  const special = FILENAME_SPECIALS[basename];
  if (special) return { Icon: CATEGORY_ICONS[special], category: special };

  const ext = extensionOf(name);
  for (const spec of CATEGORIES) {
    if (spec.extensions.includes(ext)) return { Icon: spec.Icon, category: spec.category };
  }
  return { Icon: File, category: "default" };
}
