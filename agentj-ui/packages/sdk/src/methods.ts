// GENERATED — do not edit manually.
// Source: protocol/methods.json
// Generator: agentj-ui/packages/sdk/generate-methods.mjs
export const METHODS = {
  initialize: "initialize",
  initialized: "initialized",
  shutdown: "shutdown",
  exit: "exit",
  sessionNew: "session/new",
  sessionRun: "session/run",
  sessionStatus: "session/status",
  sessionArchive: "session/archive",
  sessionDelete: "session/delete",
  sessionSubscribe: "session/subscribe",
  sessionUnsubscribe: "session/unsubscribe",
  kernelStatus: "kernel/status",
  cancelRequest: "$/cancelRequest",
  progress: "$/progress",
  permissionRequest: "$/permissionRequest",
  permissionRespond: "permission/respond",
} as const;

export type MethodName = (typeof METHODS)[keyof typeof METHODS];
