export { RpcClient } from "./client.js";

export type { RpcResponse, InitializeResult } from "./client.js";
export { ResponseRouter } from "./router.js";
export type { NotificationHandler, RpcRequest } from "./router.js";
export type { RpcTransport } from "./router.js";
export { WebSocketTransport } from "./transports/websocket.js";
export type { TransportStatus } from "./transports/websocket.js";

export { PROTOCOL_VERSION, VERSION, resolveWsUrl } from "./config.js";
export type { WsLocation } from "./config.js";
export { BLANK_CHARS, isBlankText } from "./blankText.js";
export { METHODS } from "./methods.js";
export { parseProgress } from "./progress.js";
export type { ProgressFrame, UsageInfo, TodoFrame, TodoItemFrame, FileChangeFrame } from "./progress.js";
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
} from "./files.js";
export type {
  FileNode,
  GitLetter,
  RawFileEntry,
  RowParts,
  SearchEntry,
  VisibleRow,
  VisibleRowNote,
} from "./files.js";
export { toolSummary } from "./toolSummary.js";
export { formatContextWindow } from "./modelMeta.js";
export { buildTrend, sparkline } from "./usageTrend.js";
export type { TrendPoint, UsageSampleLike } from "./usageTrend.js";
// TcpTransport is Node-only (node:net) — import via "@agentj/sdk/src/transports/tcp.js" for Node consumers.
