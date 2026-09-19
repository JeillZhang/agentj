import { useState } from "react";
import { Plug, Link, Key, Eye, EyeOff } from "lucide-react";
import { loadConnection } from "./connectionStore";

type Props = {
  onConnect: (host: string, port: string, token: string) => void;
};

export default function ConnectionBar({ onConnect }: Props) {
  const [stored] = useState(loadConnection);
  const [host, setHost] = useState(stored?.host ?? "");
  const [port, setPort] = useState(stored?.port ?? "");
  const [token, setToken] = useState(stored?.token ?? "");
  const [showToken, setShowToken] = useState(false);
  const [hostError, setHostError] = useState<string | null>(null);

  function validate(): boolean {
    if (!host.trim()) {
      setHostError("host required");
      return false;
    }
    setHostError(null);
    return true;
  }

  function handleConnect() {
    if (!validate()) return;
    onConnect(host, port.trim() || "4096", token);
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
            <p id="host-error" role="alert" className="aj-error">
              {hostError}
            </p>
          ) : (
            <p id="aj-host-helper" className="aj-helper">
              required
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
            onChange={(e) => setPort(e.target.value)}
            placeholder="4096"
            aria-describedby="aj-port-helper"
          />
          <p id="aj-port-helper" className="aj-helper">
            optional
          </p>
        </div>
        <div className="aj-field">
          <label htmlFor="aj-token" className="aj-label">
            <Key aria-hidden="true" size={16} />
            Token
          </label>
          <div className="aj-input-wrap">
            <input
              id="aj-token"
              className="aj-input"
              type={showToken ? "text" : "password"}
              value={token}
              onChange={(e) => setToken(e.target.value)}
              placeholder="token"
              aria-describedby="aj-token-helper"
              autoComplete="off"
            />
            <button
              type="button"
              className="aj-input-addon"
              aria-label={showToken ? "hide password" : "show password"}
              aria-pressed={showToken}
              onClick={() => setShowToken((v) => !v)}
            >
              {showToken ? <Eye size={16} aria-hidden="true" /> : <EyeOff size={16} aria-hidden="true" />}
            </button>
          </div>
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
