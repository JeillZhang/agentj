// agentj-ui/apps/tui/src/files.ts
// Thin re-export of the shared pure helpers in @agentj/sdk — the TUI and the Web file
// tree must not carry two implementations. No function bodies live here.
//
// NOTE: the shared `moveSelection` clamps with no wrap-around (TUI semantics). The Web
// sidebar keeps its own wrap-aware `moveSelection` in hooks/useRovingFocus.ts and must
// NOT be swapped for this one.
export {
  MAX_INDENT_DEPTH,
  buildFileNode,
  buildSearchIndex,
  clampSelection,
  flattenVisible,
  formatFileRowParts,
  formatSearchRowParts,
  isEmptyExpandedDir,
  moveSelection,
  parentPath,
  statusLetterColor,
  toGitLetter,
  truncate,
  updateNode,
} from "@agentj/sdk";
export type {
  FileNode,
  GitLetter,
  RawFileEntry,
  RowParts,
  SearchEntry,
  VisibleRow,
  VisibleRowNote,
} from "@agentj/sdk";
