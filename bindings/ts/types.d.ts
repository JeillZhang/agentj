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
  [k: string]: unknown;
}

export interface InitializeResult {
  capabilities?: unknown;
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
  modules: ModuleInfo[];
  version: string;
  [k: string]: unknown;
}
export interface ModuleInfo {
  id: string;
  state: ModuleState;
  [k: string]: unknown;
}




export interface NotificationMessage {
  jsonrpc: string;
  method: string;
  params?: unknown;
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
    };

export interface ProgressParams {
  token: ProgressToken;
  value: ProgressValue;
  [k: string]: unknown;
}

/**
 * Streaming progress token — independent of the request id (design 决策 #3).
 */

/**
 * `$/progress` payload, discriminated by `kind` (design 决策 #3).
 * An unknown `kind` is ignorable by consumers (per `$/`-prefix convention,
 * design 决策 #5) — consumer-side behavior documented in the spec, deferred to v0.1.
 */


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




export interface SessionIdParams {
  session_id: string;
  [k: string]: unknown;
}

export interface SessionNewResult {
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
  tool_calls?: ToolCall[] | null;
  [k: string]: unknown;
}

export type SessionStatus = "active" | "archived" | "deleted";


export interface SessionStatusResult {
  status: SessionStatus;
  [k: string]: unknown;
}


export interface ToolDefinition {
  description: string;
  input_schema: unknown;
  name: string;
  [k: string]: unknown;
}
