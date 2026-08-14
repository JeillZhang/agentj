import type { InitializeParams, InitializeResult, SessionRunParams, SessionRunResult } from "./types";

const params: InitializeParams = {
  protocol_version: "0.1.0",
  client_info: { name: "cli", version: "0.1.0" },
};
const run: SessionRunParams = { session_id: "s1", content: "hi" };
const result: InitializeResult = {
  protocol_version: "0.1.0",
  server_info: { name: "agentj", version: "0.1.0" },
};
const runResult: SessionRunResult = { content: "done" };
void [params, run, result, runResult];
