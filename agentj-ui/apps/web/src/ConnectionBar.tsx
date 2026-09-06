import { useState } from "react";
import { Plug, Link, Key } from "lucide-react";

type Props = {
  onConnect: (host: string, port: string, token: string) => void;
};

export default function ConnectionBar({ onConnect }: Props) {
  const [host, setHost] = useState("");
  const [port, setPort] = useState("");
  const [token, setToken] = useState("");
  const [hostError, setHostError] = useState<string | null>(null);
  const [portError, setPortError] = useState<string | null>(null);

  function validate(): boolean {
    let hasError = false;
    if (!host.trim()) {
      setHostError("host required");
      hasError = true;
    } else {
      setHostError(null);
    }
    if (!port.trim()) {
      setPortError("port required");
      hasError = true;
    } else {
      setPortError(null);
    }
    return !hasError;
  }

  function handleConnect() {
    validate();
    onConnect(host, port, token);
  }

  return (
    <section aria-label="connection" className="aj-connection-card">
      <div className="aj-connection-header">
        <Plug aria-hidden="true" size={16} />
        <span>Connection</span>
      </div>
      <div className="aj-connection-fields">
        <div className="aj-field">
          <label htmlFor="aj-host" className="aj-label">
            <Link aria-hidden="true" size={16} />
            Host
          </label>
          <input
            id="aj-host"
            className="aj-input"
            value={host}
            onChange={(e) => {
              setHost(e.target.value);
              if (hostError && e.target.value.trim()) setHostError(null);
            }}
            onBlur={() => {
              if (!host.trim()) setHostError("host required");
              else setHostError(null);
            }}
            placeholder="127.0.0.1"
            aria-describedby={hostError ? "host-error" : "aj-host-helper"}
            aria-invalid={hostError ? true : undefined}
          />
          {hostError ? (
            <>
              <p id="host-error" role="alert" className="aj-error">
                {hostError}
              </p>
              <span id="aj-host-error" style={{ display: "none" }}>
                {hostError}
              </span>
            </>
          ) : (
            <p id="aj-host-helper" className="aj-helper">
              e.g. 127.0.0.1
            </p>
          )}
        </div>
        <div className="aj-field">
          <label htmlFor="aj-port" className="aj-label">
            <Link aria-hidden="true" size={16} />
            Port
          </label>
          <input
            id="aj-port"
            className="aj-input"
            value={port}
            onChange={(e) => {
              setPort(e.target.value);
              if (portError && e.target.value.trim()) setPortError(null);
            }}
            onBlur={() => {
              if (!port.trim()) setPortError("port required");
              else setPortError(null);
            }}
            placeholder="4096"
            aria-describedby={portError ? "port-error" : "aj-port-helper"}
            aria-invalid={portError ? true : undefined}
          />
          {portError ? (
            <>
              <p id="port-error" role="alert" className="aj-error">
                {portError}
              </p>
              <span id="aj-port-error" style={{ display: "none" }}>
                {portError}
              </span>
            </>
          ) : (
            <p id="aj-port-helper" className="aj-helper">
              e.g. 4096
            </p>
          )}
        </div>
        <div className="aj-field">
          <label htmlFor="aj-token" className="aj-label">
            <Key aria-hidden="true" size={16} />
            Token
          </label>
          <input
            id="aj-token"
            className="aj-input"
            value={token}
            onChange={(e) => setToken(e.target.value)}
            placeholder="token"
            aria-describedby="aj-token-helper"
          />
          <p id="aj-token-helper" className="aj-helper">
            optional, leave empty if none
          </p>
        </div>
      </div>
      <button type="button" className="aj-button" onClick={handleConnect}>
        Connect
      </button>
    </section>
  );
}
