type PermissionArgs = unknown;

export type ApprovalRequest = {
  request_id: string;
  tool: string;
  args: PermissionArgs;
  session_id: string | null;
  reason?: string;
};

type Props = {
  request: ApprovalRequest | null;
  error: string | null;
  onRespond: (decision: string, allowAlways: boolean) => Promise<void>;
  onClose: () => void;
};

function formatArgs(args: PermissionArgs): string {
  if (args === null || args === undefined) return "";
  if (typeof args === "string") return args;
  try {
    return JSON.stringify(args);
  } catch {
    return String(args);
  }
}

export default function ApprovalDialog({ request, error, onRespond, onClose }: Props) {
  if (!request) return null;
  const argsText = formatArgs(request.args);
  return (
    <div role="dialog" aria-label="permission request" aria-modal="true">
      <h2>Permission request</h2>
      <p>tool: {request.tool}</p>
      {argsText && <p>args: {argsText}</p>}
      {request.session_id && <p>session: {request.session_id}</p>}
      {request.reason && <p>reason: {request.reason}</p>}
      {error && (
        <p role="alert">{error}</p>
      )}
      <div>
        <button type="button" onClick={() => void onRespond("allow", false)}>Allow</button>
        <button type="button" onClick={() => void onRespond("deny", false)}>Deny</button>
        <button type="button" onClick={() => void onRespond("allow", true)}>Allow All</button>
        {error && (
          <button type="button" onClick={onClose}>Close</button>
        )}
      </div>
    </div>
  );
}
