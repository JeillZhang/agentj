/**
 * Why a key could not be linked. `kind` stays a `String` for forward compatibility
 * (precedent: [`ProviderUsageError::kind`]); `message` is always kernel-authored.
 * @since 0.18.0
 */
export interface ApiKeyLinkError {
  kind: string;
  message: string;
  [k: string]: unknown;
}

/**
 * Verification outcome of a stored key. `Unknown` tolerates forward values.
 * @since 0.18.0
 */
export type ApiKeyOutcome = "verified" | "unverified" | "rejected";

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
 * `auth/clearCredential` params. @since 0.18.0
 */
export interface AuthClearCredentialParams {
  /**
   * Provider name; omitted means the configured default provider. @since 0.18.0
   */
  provider?: string | null;
  [k: string]: unknown;
}

/**
 * `auth/clearCredential` result; `cleared: false` means no stored entry existed.
 * @since 0.18.0
 */
export interface AuthClearCredentialResult {
  cleared: boolean;
  provider: string;
  [k: string]: unknown;
}

/**
 * `auth/devicePoll` params. @since 0.17.0
 */
export interface AuthDevicePollParams {
  device_code: string;
  /**
   * Provider name; omitted means the configured default provider. @since 0.17.0
   */
  provider?: string | null;
  [k: string]: unknown;
}

/**
 * Credential kind. `Unknown` tolerates forward values. @since 0.17.0
 */
export type CredentialKind = "api_key" | "oauth";
/**
 * Where the credential came from. `Unknown` tolerates forward values. @since 0.17.0
 */
export type CredentialSource = "flag" | "env" | "file";
/**
 * Device-flow poll state. `Unknown` tolerates forward values. @since 0.17.0
 */
export type DeviceAuthState = "pending" | "slow_down" | "authorized" | "expired" | "denied" | "error";

/**
 * `auth/devicePoll` result — the poll state, never a raw token on failure states.
 * @since 0.17.0
 */
export interface AuthDevicePollResult {
  credential?: CredentialInfo | null;
  /**
   * Next poll interval in seconds (present for `slow_down`). @since 0.17.0
   */
  interval?: number | null;
  message?: string | null;
  state: DeviceAuthState;
  [k: string]: unknown;
}
/**
 * Redacted credential projection. `key_hint` is the api_key's last four characters
 * (`…1234`); OAuth credentials are identified by `org_id`/`expires_at` instead.
 * @since 0.17.0
 */
export interface CredentialInfo {
  expires_at?: number | null;
  expiring_soon: boolean;
  key_hint?: string | null;
  kind: CredentialKind;
  org_id?: string | null;
  source: CredentialSource;
  [k: string]: unknown;
}

/**
 * `auth/deviceStart` params. @since 0.17.0
 */
export interface AuthDeviceStartParams {
  /**
   * Provider name; omitted means the configured default provider. @since 0.17.0
   */
  provider?: string | null;
  [k: string]: unknown;
}

/**
 * `auth/deviceStart` result (RFC 8628 device authorization response). @since 0.17.0
 */
export interface AuthDeviceStartResult {
  device_code: string;
  expires_in: number;
  interval: number;
  provider: string;
  user_code: string;
  verification_uri: string;
  [k: string]: unknown;
}

/**
 * `auth/setApiKey` params. @since 0.18.0
 */
export interface AuthSetApiKeyParams {
  api_key: string;
  /**
   * Free-form user label stored next to the credential. @since 0.18.0
   */
  label?: string | null;
  /**
   * Provider name; omitted means the configured default provider. @since 0.18.0
   */
  provider?: string | null;
  [k: string]: unknown;
}

/**
 * Credential kind. `Unknown` tolerates forward values. @since 0.17.0
 */
/**
 * Where the credential came from. `Unknown` tolerates forward values. @since 0.17.0
 */
/**
 * Verification outcome of a stored key. `Unknown` tolerates forward values.
 * @since 0.18.0
 */
/**
 * Catalog tier (account → live → overlay; static for non-go providers).
 * `Unknown` tolerates forward values; the schema lists only the known tiers.
 * @since 0.17.0
 */
export type ModelSource = "account" | "live" | "overlay" | "static";

/**
 * `auth/setApiKey` result. Never carries a full key — see [`CredentialInfo`];
 * `effective_source` is where the live credential actually resolved from.
 * @since 0.18.0
 */
export interface AuthSetApiKeyResult {
  credential?: CredentialInfo | null;
  effective_source: CredentialSource;
  error?: ApiKeyLinkError | null;
  fetched_at: number;
  models: ModelEntry[];
  outcome: ApiKeyOutcome;
  provider: string;
  saved: boolean;
  source?: ModelSource | null;
  [k: string]: unknown;
}
/**
 * Redacted credential projection. `key_hint` is the api_key's last four characters
 * (`…1234`); OAuth credentials are identified by `org_id`/`expires_at` instead.
 * @since 0.17.0
 */
/**
 * Why a key could not be linked. `kind` stays a `String` for forward compatibility
 * (precedent: [`ProviderUsageError::kind`]); `message` is always kernel-authored.
 * @since 0.18.0
 */
/**
 * One catalog entry. `verified: false` means the route was synthesized, not known.
 * @since 0.17.0
 */
export interface ModelEntry {
  /**
   * Context window in tokens; absent when upstream metadata does not publish a window.
   * @since 0.20.0
   */
  context_window?: number | null;
  format: string;
  id: string;
  verified: boolean;
  [k: string]: unknown;
}

/**
 * `auth/status` params. @since 0.17.0
 */
export interface AuthStatusParams {
  /**
   * Provider name; omitted means the configured default provider. @since 0.17.0
   */
  provider?: string | null;
  [k: string]: unknown;
}

/**
 * Credential kind. `Unknown` tolerates forward values. @since 0.17.0
 */
/**
 * Where the credential came from. `Unknown` tolerates forward values. @since 0.17.0
 */

/**
 * `auth/status` result. Never carries a full key/token — see [`CredentialInfo`].
 * @since 0.17.0
 */
export interface AuthStatusResult {
  authenticated: boolean;
  credential?: CredentialInfo | null;
  models_count: number;
  provider: string;
  [k: string]: unknown;
}
/**
 * Redacted credential projection. `key_hint` is the api_key's last four characters
 * (`…1234`); OAuth credentials are identified by `org_id`/`expires_at` instead.
 * @since 0.17.0
 */

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

/**
 * Credential kind. `Unknown` tolerates forward values. @since 0.17.0
 */
/**
 * Where the credential came from. `Unknown` tolerates forward values. @since 0.17.0
 */

/**
 * Redacted credential projection. `key_hint` is the api_key's last four characters
 * (`…1234`); OAuth credentials are identified by `org_id`/`expires_at` instead.
 * @since 0.17.0
 */

/**
 * Credential kind. `Unknown` tolerates forward values. @since 0.17.0
 */

/**
 * Where the credential came from. `Unknown` tolerates forward values. @since 0.17.0
 */

/**
 * Device-flow poll state. `Unknown` tolerates forward values. @since 0.17.0
 */

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

/**
 * One catalog entry. `verified: false` means the route was synthesized, not known.
 * @since 0.17.0
 */

/**
 * `model/list` params. @since 0.17.0
 */
export interface ModelListParams {
  /**
   * Provider name; omitted means the configured default provider. @since 0.17.0
   */
  provider?: string | null;
  /**
   * Bypass the 60s in-process reuse window. @since 0.17.0
   */
  refresh?: boolean | null;
  [k: string]: unknown;
}

/**
 * Catalog tier (account → live → overlay; static for non-go providers).
 * `Unknown` tolerates forward values; the schema lists only the known tiers.
 * @since 0.17.0
 */

/**
 * `model/list` result — catalog plus the tier it came from. @since 0.17.0
 */
export interface ModelListResult {
  default_model: string;
  /**
   * Unix epoch seconds when the catalog was fetched. @since 0.17.0
   */
  fetched_at: number;
  models: ModelEntry[];
  provider: string;
  source: ModelSource;
  [k: string]: unknown;
}
/**
 * One catalog entry. `verified: false` means the route was synthesized, not known.
 * @since 0.17.0
 */

/**
 * Catalog tier (account → live → overlay; static for non-go providers).
 * `Unknown` tolerates forward values; the schema lists only the known tiers.
 * @since 0.17.0
 */




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
      /**
       * Characters the kernel dropped from this result (design ㊵). Absent on the wire
       * when nothing was truncated, so an untruncated frame is byte-identical to 0.20.0.
       * @since 0.21.0
       */
      truncated_chars?: number | null;
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
 * Domain failure carried inside a successful `provider/usage` response.
 * `kind ∈ {no_credential, expired, unauthorized, entitlement, unavailable, network}`.
 * @since 0.17.0
 */
export interface ProviderUsageError {
  kind: string;
  message: string;
  [k: string]: unknown;
}

/**
 * Quota window kind. `Unknown` tolerates forward values. @since 0.17.0
 */
export type UsageWindowKind = "rolling" | "weekly" | "monthly";

/**
 * Query parameters for `provider/usageHistory`. Every field is optional so an empty
 * `params` object means "default provider, all kinds, full retention window". @since 0.20.0
 */
export interface ProviderUsageHistoryParams {
  kind?: UsageWindowKind | null;
  limit?: number | null;
  provider?: string | null;
  since?: number | null;
  [k: string]: unknown;
}

/**
 * Quota window kind. `Unknown` tolerates forward values. @since 0.17.0
 */
/**
 * Window status. `Unknown` tolerates forward values. @since 0.17.0
 */
export type UsageWindowStatus = "ok" | "rate-limited";

/**
 * Result of `provider/usageHistory`, samples ordered by `at` ascending. @since 0.20.0
 */
export interface ProviderUsageHistoryResult {
  provider: string;
  /**
   * Retention actually applied by the kernel, so a client can label the axis.
   */
  retention_days: number;
  samples: UsageSample[];
  [k: string]: unknown;
}
/**
 * One persisted sampling of every usage window the provider reported. @since 0.20.0
 */
export interface UsageSample {
  at: number;
  windows: UsageWindow[];
  [k: string]: unknown;
}
/**
 * One quota window. `percent` is the *used* percentage. @since 0.17.0
 */
export interface UsageWindow {
  kind: UsageWindowKind;
  /**
   * `None` when upstream's value was absent or not an integer. @since 0.17.0
   */
  percent?: number | null;
  /**
   * Omitted when `percent == 0` (upstream placeholder is dropped). @since 0.17.0
   */
  resets_at?: string | null;
  status: UsageWindowStatus;
  [k: string]: unknown;
}

/**
 * `provider/usage` params. @since 0.17.0
 */
export interface ProviderUsageParams {
  /**
   * Provider name; omitted means the configured default provider. @since 0.17.0
   */
  provider?: string | null;
  /**
   * Bypass the 300s in-process cache. @since 0.17.0
   */
  refresh?: boolean | null;
  [k: string]: unknown;
}

/**
 * Quota window kind. `Unknown` tolerates forward values. @since 0.17.0
 */
/**
 * Window status. `Unknown` tolerates forward values. @since 0.17.0
 */

/**
 * `provider/usage` result. Domain failures travel in `error` (success response).
 * @since 0.17.0
 */
export interface ProviderUsageResult {
  error?: ProviderUsageError | null;
  /**
   * Unix epoch seconds when the snapshot was fetched. @since 0.17.0
   */
  fetched_at: number;
  provider: string;
  /**
   * `true` when `windows` is a reused earlier snapshot (error path). @since 0.17.0
   */
  stale: boolean;
  /**
   * Client-side warning threshold in percent (1..=100). @since 0.17.0
   */
  warn_percent: number;
  windows: UsageWindow[];
  [k: string]: unknown;
}
/**
 * Domain failure carried inside a successful `provider/usage` response.
 * `kind ∈ {no_credential, expired, unauthorized, entitlement, unavailable, network}`.
 * @since 0.17.0
 */
/**
 * One quota window. `percent` is the *used* percentage. @since 0.17.0
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
    }
  | {
      kind: "run_model";
      model?: string | null;
      provider?: string | null;
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
  /**
   * Per-run model id override, e.g. "opencode-go/kimi-k3". Absent = config
   * model.default_model. @since 0.16.0
   */
  model?: string | null;
  progress_token?: ProgressToken | null;
  /**
   * Per-run provider name override (registered provider name). Absent = default
   * provider. @since 0.16.0
   */
  provider?: string | null;
  session_id: string;
  [k: string]: unknown;
}

export interface SessionRunResult {
  content: string;
  /**
   * Effective model of the turn. @since 0.19.0
   */
  model?: string | null;
  /**
   * Effective provider of the turn. @since 0.19.0
   */
  provider?: string | null;
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


/**
 * Quota window kind. `Unknown` tolerates forward values. @since 0.17.0
 */
/**
 * Window status. `Unknown` tolerates forward values. @since 0.17.0
 */

/**
 * One persisted sampling of every usage window the provider reported. @since 0.20.0
 */
/**
 * One quota window. `percent` is the *used* percentage. @since 0.17.0
 */

/**
 * Quota window kind. `Unknown` tolerates forward values. @since 0.17.0
 */
/**
 * Window status. `Unknown` tolerates forward values. @since 0.17.0
 */

/**
 * One quota window. `percent` is the *used* percentage. @since 0.17.0
 */

/**
 * Quota window kind. `Unknown` tolerates forward values. @since 0.17.0
 */

/**
 * Window status. `Unknown` tolerates forward values. @since 0.17.0
 */
