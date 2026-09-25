/**
 * One selectable answer option. @since 0.13.0
 */
export interface AskOption {
  /**
   * Optional hint rendered with the option. @since 0.13.0
   */
  hint?: string | null;
  id: string;
  label: string;
  [k: string]: unknown;
}

/**
 * `$/askQuestion` params (server → owner client, 2–8 options). @since 0.13.0
 */
export interface AskQuestionParams {
  /**
   * Always serialized; omitted on the wire means false. @since 0.13.0
   */
  allow_multiple?: boolean;
  /**
   * Always serialized; omitted on the wire means true. @since 0.13.0
   */
  allow_other?: boolean;
  /**
   * Optional short header for the question card. @since 0.13.0
   */
  header?: string | null;
  options: AskOption[];
  question: string;
  question_id: string;
  session_id: string;
  /**
   * Timeout in seconds; `None` = no timeout (timeout settles as skipped).
   * @since 0.13.0
   */
  timeout_s?: number | null;
  [k: string]: unknown;
}
/**
 * One selectable answer option. @since 0.13.0
 */

export interface Attachment {
  content?: string | null;
  mime?: string | null;
  path?: string | null;
  [k: string]: unknown;
}

/**
 * JSON-RPC 2.0 request/notification id. `Null` is used only in error responses
 * where the request id could not be determined (e.g. parse error — JSON-RPC 2.0 §5).
 */
export type Id = number | string | null;

export interface CancelParams {
  id: Id;
  /**
   * Target session for the cancel (design G2). Additive: absent on legacy
   * clients — the server falls back to a single-inflight session.
   */
  session_id?: string | null;
  [k: string]: unknown;
}

export type Role = "system" | "user" | "assistant" | "tool";

export interface ChatMessage {
  content: string;
  role: Role;
  tool_call_id?: string | null;
  tool_calls?: ToolCall[];
  [k: string]: unknown;
}
export interface ToolCall {
  arguments: unknown;
  id: string;
  name: string;
  [k: string]: unknown;
}

export interface ClientInfo {
  name: string;
  version: string;
  [k: string]: unknown;
}

export interface ErrorObject {
  code: number;
  data?: unknown;
  message: string;
  [k: string]: unknown;
}

/**
 * Change operation of a `FileChange`. `Unknown` tolerates forward values
 * (serde(other)); schema lists only the known values. @since 0.13.0
 */
export type FileChangeKind = "add" | "modify" | "delete";
/**
 * Source of a FileChange: write produces LCS diff, bash produces counts only. @since 0.15.0
 */
export type FileChangeSource = "write" | "bash";

/**
 * A workspace file change. @since 0.13.0
 */
export interface FileChange {
  additions: number;
  deletions: number;
  /**
   * Unified diff text; omitted when unavailable (untracked / over limit).
   */
  diff?: string | null;
  /**
   * True only when the diff was truncated; omitted otherwise.
   */
  diff_truncated?: boolean | null;
  op: FileChangeKind;
  /**
   * Path relative to the project root.
   */
  path: string;
  /**
   * Run this change belongs to. @since 0.15.0
   */
  run_id?: string | null;
  /**
   * Per-run monotonic sequence for ordering inside a run. @since 0.15.0
   */
  seq?: number | null;
  /**
   * Source of the change: write (LCS diff) or bash (numstat only). @since 0.15.0
   */
  source?: FileChangeSource | null;
  /**
   * Tool call card this change belongs to; omitted when unavailable.
   */
  tool_call_id?: string | null;
  /**
   * Original diff byte length when truncated. @since 0.15.0
   */
  total_bytes?: number | null;
  [k: string]: unknown;
}

/**
 * Change operation of a `FileChange`. `Unknown` tolerates forward values
 * (serde(other)); schema lists only the known values. @since 0.13.0
 */

/**
 * Source of a FileChange: write produces LCS diff, bash produces counts only. @since 0.15.0
 */

/**
 * Porcelain single-letter git status (M/A/U/D). `Unknown` tolerates forward
 * values (serde(other)); the schema lists only the known letters. @since 0.13.0
 */
export type GitFileStatus = "M" | "A" | "U" | "D";
/**
 * Entry kind. `Unknown` tolerates forward values (serde(other)); the schema
 * lists only the known values. @since 0.13.0
 */
export type FileKind = "file" | "dir";

/**
 * One directory entry (`path` relative to the project root, no `./` prefix).
 * @since 0.13.0
 */
export interface FileEntry {
  /**
   * Porcelain single-letter status; `None` = clean / not a git repo.
   * @since 0.13.0
   */
  git_status?: GitFileStatus | null;
  kind: FileKind;
  /**
   * Modification time in epoch milliseconds. @since 0.13.0
   */
  mtime_ms?: number | null;
  /**
   * Entry name without separators.
   */
  name: string;
  /**
   * Path relative to the project root.
   */
  path: string;
  /**
   * File size in bytes; `None` for directories. @since 0.13.0
   */
  size?: number | null;
  [k: string]: unknown;
}

/**
 * Entry kind. `Unknown` tolerates forward values (serde(other)); the schema
 * lists only the known values. @since 0.13.0
 */

/**
 * `file/list` params — enumerate a directory relative to the session
 * `project_root`. @since 0.13.0
 */
export interface FileListParams {
  /**
   * Enumeration depth 1..=8 (default 1, server-validated). @since 0.13.0
   */
  depth?: number | null;
  /**
   * Directory path relative to the project root; absent = root.
   */
  path?: string | null;
  /**
   * Case-insensitive substring filter; empty/whitespace is treated as absent. @since 0.14.0
   */
  query?: string | null;
  session_id: string;
  [k: string]: unknown;
}

/**
 * Porcelain single-letter git status (M/A/U/D). `Unknown` tolerates forward
 * values (serde(other)); the schema lists only the known letters. @since 0.13.0
 */
/**
 * Entry kind. `Unknown` tolerates forward values (serde(other)); the schema
 * lists only the known values. @since 0.13.0
 */

/**
 * `file/list` result — entries capped at 2000; `truncated` flags the cap.
 * @since 0.13.0
 */
export interface FileListResult {
  entries: FileEntry[];
  /**
   * Full match count discovered by a query scan (lower bound when truncated).
   * @since 0.14.0
   */
  total?: number | null;
  truncated: boolean;
  [k: string]: unknown;
}
/**
 * One directory entry (`path` relative to the project root, no `./` prefix).
 * @since 0.13.0
 */

/**
 * JSON-RPC 2.0 request/notification id. `Null` is used only in error responses
 * where the request id could not be determined (e.g. parse error — JSON-RPC 2.0 §5).
 */

/**
 * `initialize` request params (design 决策 #2).
 */
export interface InitializeParams {
  capabilities?: unknown;
  client_info?: ClientInfo | null;
  protocol_version: string;
  /**
   * Access token required by a token-gated resident kernel (--token).
   * Absent when the kernel runs without auth — additive.
   */
  token?: string | null;
  [k: string]: unknown;
}

export interface InitializeResult {
  capabilities?: unknown;
  /**
   * This connection's unique id in a multi-client kernel (design 决策 #6).
   * Absent in single-client (stdio) mode. Capability flag: capabilities.multi_client.
   */
  connection_id?: string | null;
  protocol_version: string;
  server_info: ServerInfo;
  [k: string]: unknown;
}
export interface ServerInfo {
  name: string;
  version: string;
  [k: string]: unknown;
}

export type ModuleState = "unloaded" | "initializing" | "ready" | "stopping" | "stopped" | "failed";

export interface KernelStatusResult {
  /**
   * Config model.default_model, e.g. "deepseek-chat". Absent when no config. @since 0.12.0
   */
  default_model?: string | null;
  modules: ModuleInfo[];
  /**
   * Provider list from config (name → base_url); models reserved, empty. @since 0.12.0
   */
  providers?: ProviderInfo[] | null;
  version: string;
  [k: string]: unknown;
}
export interface ModuleInfo {
  id: string;
  state: ModuleState;
  [k: string]: unknown;
}
export interface ProviderInfo {
  id: string;
  models: string[];
  [k: string]: unknown;
}




export interface NotificationMessage {
  jsonrpc: string;
  method: string;
  params?: unknown;
  [k: string]: unknown;
}

/**
 * Decision value in `PermissionRespondParams`. `Unknown` tolerates forward
 * values (serde(other)); schema lists only the known values.
 */
export type PermissionDecision = "allow" | "deny" | "allow_always";

/**
 * S→C notification: a tool call is awaiting user approval (`Ask` decision).
 */
export interface PermissionRequestParams {
  /**
   * Raw tool arguments (contains `cmd` for bash, `path` for file tools).
   */
  args: {
    [k: string]: unknown;
  };
  /**
   * Connection id of the client that initiated the run (design 决策 #5).
   * Absent in single-client (stdio) mode — additive.
   */
  origin_client?: string | null;
  /**
   * Human-readable reason (tool default mode / matched rule).
   */
  reason: string;
  /**
   * Opaque id decoupled from the JSON-RPC request id (like $/progress token).
   */
  request_id: string;
  /**
   * Session owning the approval (client-side UI attribution; additive).
   */
  session_id?: string | null;
  /**
   * Tool name, e.g. "bash".
   */
  tool: string;
  [k: string]: unknown;
}

/**
 * Decision value in `PermissionRespondParams`. `Unknown` tolerates forward
 * values (serde(other)); schema lists only the known values.
 */

/**
 * C→S request: the client's decision on a pending permission request.
 */
export interface PermissionRespondParams {
  decision: PermissionDecision;
  request_id: string;
  [k: string]: unknown;
}

/**
 * `permission/respond` success result — empty object.
 */
export interface PermissionRespondResult {
  [k: string]: unknown;
}

/**
 * Streaming progress token — independent of the request id (design 决策 #3).
 */
export type ProgressToken = number | string;
/**
 * `$/progress` payload, discriminated by `kind` (design 决策 #3).
 * An unknown `kind` is ignorable by consumers (per `$/`-prefix convention,
 * design 决策 #5) — consumer-side behavior documented in the spec, deferred to v0.1.
 */
export type ProgressValue =
  | {
      delta: string;
      kind: "text";
      [k: string]: unknown;
    }
  | {
      kind: "tool_call";
      tool_call: ToolCall;
      [k: string]: unknown;
    }
  | {
      content: string;
      kind: "tool_result";
      tool_call_id: string;
      [k: string]: unknown;
    }
  | {
      kind: "usage";
      usage: UsageInfo;
      [k: string]: unknown;
    }
  | {
      decision: PermissionDecision;
      kind: "permission";
      request_id: string;
      session_id?: string | null;
      tool: string;
      [k: string]: unknown;
    }
  | {
      items: TodoItem[];
      kind: "todo";
      revision: number;
      run_id?: string | null;
      session_id: string;
      [k: string]: unknown;
    }
  | {
      change: FileChange;
      kind: "file_change";
      run_id?: string | null;
      session_id: string;
      [k: string]: unknown;
    };
/**
 * Decision value in `PermissionRespondParams`. `Unknown` tolerates forward
 * values (serde(other)); schema lists only the known values.
 */
/**
 * Lifecycle status of a TODO item. `Unknown` tolerates forward values
 * (serde(other)); schema lists only the known values. @since 0.13.0
 */
export type TodoStatus = "pending" | "in_progress" | "completed" | "cancelled";
/**
 * Change operation of a `FileChange`. `Unknown` tolerates forward values
 * (serde(other)); schema lists only the known values. @since 0.13.0
 */
/**
 * Source of a FileChange: write produces LCS diff, bash produces counts only. @since 0.15.0
 */

export interface ProgressParams {
  token: ProgressToken;
  value: ProgressValue;
  [k: string]: unknown;
}
export interface UsageInfo {
  completion_tokens: number;
  prompt_tokens: number;
  [k: string]: unknown;
}
/**
 * One entry of a full TODO snapshot. @since 0.13.0
 */
export interface TodoItem {
  content: string;
  /**
   * Kernel-computed stable id (content hash; duplicates disambiguated by occurrence).
   */
  id: string;
  status: TodoStatus;
  [k: string]: unknown;
}
/**
 * A workspace file change. @since 0.13.0
 */

/**
 * Streaming progress token — independent of the request id (design 决策 #3).
 */

/**
 * `$/progress` payload, discriminated by `kind` (design 决策 #3).
 * An unknown `kind` is ignorable by consumers (per `$/`-prefix convention,
 * design 决策 #5) — consumer-side behavior documented in the spec, deferred to v0.1.
 */
/**
 * Decision value in `PermissionRespondParams`. `Unknown` tolerates forward
 * values (serde(other)); schema lists only the known values.
 */
/**
 * Lifecycle status of a TODO item. `Unknown` tolerates forward values
 * (serde(other)); schema lists only the known values. @since 0.13.0
 */
/**
 * Change operation of a `FileChange`. `Unknown` tolerates forward values
 * (serde(other)); schema lists only the known values. @since 0.13.0
 */
/**
 * Source of a FileChange: write produces LCS diff, bash produces counts only. @since 0.15.0
 */

/**
 * One entry of a full TODO snapshot. @since 0.13.0
 */
/**
 * A workspace file change. @since 0.13.0
 */

/**
 * `question/respond` params (`answers` are option ids or `other:`-prefixed
 * free text). @since 0.13.0
 */
export interface QuestionRespondParams {
  answers: string[];
  question_id: string;
  /**
   * Omitted on the wire means false. @since 0.13.0
   */
  skipped?: boolean;
  [k: string]: unknown;
}

/**
 * `question/respond` result — `false` for duplicate/unknown/settled ids.
 * @since 0.13.0
 */
export interface QuestionRespondResult {
  accepted: boolean;
  [k: string]: unknown;
}

/**
 * JSON-RPC 2.0 request/notification id. `Null` is used only in error responses
 * where the request id could not be determined (e.g. parse error — JSON-RPC 2.0 §5).
 */

export interface RequestMessage {
  id: Id;
  jsonrpc: string;
  method: string;
  params?: unknown;
  [k: string]: unknown;
}

/**
 * A JSON-RPC 2.0 response: exactly one of `result` (success) or `error` (failure).
 */
export type ResponseMessage =
  | {
      id: Id;
      jsonrpc: string;
      result: unknown;
      [k: string]: unknown;
    }
  | {
      error: ErrorObject;
      id: Id;
      jsonrpc: string;
      [k: string]: unknown;
    };
/**
 * JSON-RPC 2.0 request/notification id. `Null` is used only in error responses
 * where the request id could not be determined (e.g. parse error — JSON-RPC 2.0 §5).
 */




/**
 * Page of a session trajectory. @since 0.12.0
 */
export interface SessionHistoryParams {
  cursor?: number | null;
  limit?: number | null;
  /**
   * Filter to a single run's Todo/FileChange events. @since 0.15.0
   */
  run_id?: string | null;
  session_id: string;
  [k: string]: unknown;
}

/**
 * Kind payload of a trajectory event, discriminated by `value.kind`.
 * Forward-compat by the standard rule: consumers ignore unknown kinds, and the
 * store replay skips unparseable lines defensively (see T7) — no serde(other)
 * needed on a tagged enum with payloads, matching ProgressValue's policy.
 */
export type TrajectoryValue =
  | {
      content: string;
      kind: "user";
      [k: string]: unknown;
    }
  | {
      content: string;
      kind: "thinking";
      [k: string]: unknown;
    }
  | {
      kind: "tool_call";
      tool_call: ToolCall;
      [k: string]: unknown;
    }
  | {
      content: string;
      kind: "tool_result";
      tool_call_id: string;
      [k: string]: unknown;
    }
  | {
      decision: PermissionDecision;
      kind: "permission";
      request_id: string;
      tool: string;
      [k: string]: unknown;
    }
  | {
      kind: "usage";
      usage: UsageInfo;
      [k: string]: unknown;
    }
  | {
      kind: "error";
      message: string;
      [k: string]: unknown;
    }
  | {
      items: TodoItem[];
      kind: "todo";
      revision: number;
      run_id?: string | null;
      [k: string]: unknown;
    }
  | {
      change: FileChange;
      kind: "file_change";
      run_id?: string | null;
      [k: string]: unknown;
    }
  | {
      allow_multiple: boolean;
      allow_other: boolean;
      header?: string | null;
      kind: "ask_question";
      options: AskOption[];
      question: string;
      question_id: string;
      [k: string]: unknown;
    }
  | {
      answers: string[];
      kind: "question_answer";
      question_id: string;
      skipped: boolean;
      [k: string]: unknown;
    };
/**
 * Decision value in `PermissionRespondParams`. `Unknown` tolerates forward
 * values (serde(other)); schema lists only the known values.
 */
/**
 * Lifecycle status of a TODO item. `Unknown` tolerates forward values
 * (serde(other)); schema lists only the known values. @since 0.13.0
 */
/**
 * Change operation of a `FileChange`. `Unknown` tolerates forward values
 * (serde(other)); schema lists only the known values. @since 0.13.0
 */
/**
 * Source of a FileChange: write produces LCS diff, bash produces counts only. @since 0.15.0
 */

/**
 * @since 0.12.0
 */
export interface SessionHistoryResult {
  events: TrajectoryEvent[];
  next_cursor?: number | null;
  [k: string]: unknown;
}
export interface TrajectoryEvent {
  /**
   * Monotonic per-session sequence number; page cursor = last seen seq + 1.
   */
  seq: number;
  session_id: string;
  /**
   * Unix epoch **milliseconds**.
   */
  ts: number;
  value: TrajectoryValue;
  [k: string]: unknown;
}
/**
 * One entry of a full TODO snapshot. @since 0.13.0
 */
/**
 * A workspace file change. @since 0.13.0
 */
/**
 * One selectable answer option. @since 0.13.0
 */

export interface SessionIdParams {
  session_id: string;
  [k: string]: unknown;
}

/**
 * @since 0.12.0
 */
export interface SessionListParams {
  [k: string]: unknown;
}

export type SessionStatus = "active" | "archived" | "deleted";

/**
 * @since 0.12.0
 */
export interface SessionListResult {
  sessions: SessionSummary[];
  [k: string]: unknown;
}
/**
 * One row of `session/list`. @since 0.12.0
 */
export interface SessionSummary {
  preview?: string | null;
  session_id: string;
  state: SessionStatus;
  updated_at: number;
  [k: string]: unknown;
}

/**
 * `session/new` params. `project_root` is an absolute directory; when omitted
 * the session defaults to the process CWD. @since 0.13.0
 */
export interface SessionNewParams {
  project_root?: string | null;
  [k: string]: unknown;
}

export interface SessionNewResult {
  /**
   * Effective canonicalized session root (cwd fallback). @since 0.14.0
   */
  project_root?: string | null;
  session_id: string;
  [k: string]: unknown;
}

/**
 * Streaming progress token — independent of the request id (design 决策 #3).
 */

export interface SessionRunParams {
  attachments?: Attachment[] | null;
  content: string;
  progress_token?: ProgressToken | null;
  session_id: string;
  [k: string]: unknown;
}

export interface SessionRunResult {
  content: string;
  /**
   * Run id that scopes this turn's todo/file_change events. @since 0.15.0
   */
  run_id?: string | null;
  tool_calls?: ToolCall[] | null;
  usage?: UsageInfo | null;
  [k: string]: unknown;
}



export interface SessionStatusResult {
  /**
   * Effective canonicalized session root (cwd fallback). @since 0.14.0
   */
  project_root?: string | null;
  status: SessionStatus;
  [k: string]: unknown;
}

/**
 * Streaming progress token — independent of the request id (design 决策 #3).
 */

/**
 * `session/subscribe` params — subscribe to a session's running progress
 * (design 决策 #4: broadcast subscription). `$/progress` broadcasts to all subscribers.
 */
export interface SessionSubscribeParams {
  progress_token?: ProgressToken | null;
  session_id: string;
  [k: string]: unknown;
}


/**
 * One row of `session/list`. @since 0.12.0
 */

/**
 * Lifecycle status of a TODO item. `Unknown` tolerates forward values
 * (serde(other)); schema lists only the known values. @since 0.13.0
 */

/**
 * One entry of a full TODO snapshot. @since 0.13.0
 */

/**
 * Lifecycle status of a TODO item. `Unknown` tolerates forward values
 * (serde(other)); schema lists only the known values. @since 0.13.0
 */


export interface ToolDefinition {
  description: string;
  input_schema: unknown;
  name: string;
  [k: string]: unknown;
}

