import { useCallback, useEffect, useState } from "react";
import { useKeyboard } from "@opentui/react";
import { METHODS, RpcClient } from "@agentj/sdk";
import type { NotificationHandler } from "@agentj/sdk";

export type PermissionRequest = {
  request_id: string;
  tool: string;
  args: unknown;
  reason?: string;
  session_id?: string | null;
  sessionId?: string | null;
};

let patched = false;
const handlerMap = new WeakMap<RpcClient, NotificationHandler[]>();

function ensurePatch(): void {
  if (patched) return;
  patched = true;
  const proto = RpcClient.prototype as unknown as {
    setNotificationHandler: (h: NotificationHandler) => void;
  };
  const orig = proto.setNotificationHandler;
  proto.setNotificationHandler = function (
    this: RpcClient,
    handler: NotificationHandler,
  ) {
    let list = handlerMap.get(this);
    if (!list) {
      list = [];
      handlerMap.set(this, list);
      orig.call(this, (method: string, params: unknown) => {
        const cur = handlerMap.get(this);
        cur?.forEach((fn) => {
          try {
            fn(method, params);
          } catch {}
        });
      });
    }
    list.push(handler);
  };
}

export function usePermission(
  client: RpcClient | null,
  onRequest: (p: PermissionRequest) => void,
): void {
  useEffect(() => {
    if (!client) return;
    ensurePatch();
    const handler: NotificationHandler = (method, params) => {
      if (method === METHODS.permissionRequest) onRequest(params as PermissionRequest);
    };
    client.setNotificationHandler(handler);
  }, [client, onRequest]);
}

export function usePermissionState(client: RpcClient | null) {
  const [pending, setPending] = useState<PermissionRequest | null>(null);
  usePermission(client, setPending);
  const respond = useCallback(
    async (payload: { decision: string }) => {
      if (!client || !pending) return;
      try {
        await client.request(METHODS.permissionRespond, {
          request_id: pending.request_id,
          decision: payload.decision,
        });
      } catch {}
      setPending(null);
    },
    [client, pending],
  );
  const clear = useCallback(() => setPending(null), []);
  return { pending, respond, clear };
}


function formatArgs(args: unknown): string {
  if (typeof args === "string") return args;
  if (args !== null && typeof args === "object") {
    try {
      const s = JSON.stringify(args);
      return s.length > 200 ? `${s.slice(0, 200)}…` : s;
    } catch {
      return String(args);
    }
  }
  return String(args ?? "");
}

export type PermissionDialogProps = {
  tool: string;
  args: unknown;
  sessionId?: string | null;
  session_id?: string | null;
  requestId?: string;
  request_id?: string;
  onRespond: (payload: {
    decision: string;
    request_id?: string;
    requestId?: string;
  }) => void;
  onClose?: () => void;
};

export function PermissionDialog(props: PermissionDialogProps): React.ReactNode {
  const sessionId = props.sessionId ?? props.session_id ?? null;
  const [focus, setFocus] = useState(0);
  const argsText = formatArgs(props.args);

  const handleRespond = useCallback(
    (idx: number) => {
      let p: { decision: string };
      if (idx === 0) p = { decision: "allow" };
      else if (idx === 1) p = { decision: "deny" };
      else p = { decision: "allow_always" };
      const rid = props.requestId ?? props.request_id;
      if (rid) props.onRespond({ ...p, request_id: rid, requestId: rid });
      else props.onRespond(p);
    },
    [props],
  );

  useKeyboard((key) => {
    const n = (key.name ?? "").toLowerCase();
    const seq = key.sequence ?? "";
    const isTab = n === "tab" || seq === "\t";
    const isEnter =
      n === "return" || n === "enter" || seq === "\r" || seq === "\n";
    const isEsc = n === "escape" || seq === "\x1b";
    if (isTab) {
      const shift = !!key.shift;
      setFocus((v) => (shift ? (v + 2) % 3 : (v + 1) % 3));
      return;
    }
    if (isEnter) {
      handleRespond(focus);
      return;
    }
    if (isEsc) props.onClose?.();
  });

  return (
    <box flexDirection="column" padding={1} border borderStyle="single" width={60}>
      <text>Permission required</text>
      <text>tool: {props.tool}</text>
      <text>args: {argsText}</text>
      {sessionId ? <text>session: {sessionId}</text> : <text>session: -</text>}
      <text>{focus === 0 ? "▶ " : "  "}Allow</text>
      <text>{focus === 1 ? "▶ " : "  "}Deny</text>
      <text>{focus === 2 ? "▶ " : "  "}AllowAll</text>
    </box>
  );
}

export function permissionRespondPayload(
  requestId: string,
  decision: string,
  allowAlways?: boolean,
): Record<string, unknown> {
  return { request_id: requestId, decision: allowAlways ? "allow_always" : decision };
}
