import { useState } from "react";

type Props = {
  onConnect: (host: string, port: string, token: string) => void;
};

export default function ConnectionBar({ onConnect }: Props) {
  const [host, setHost] = useState("");
  const [port, setPort] = useState("");
  const [token, setToken] = useState("");

  return (
    <section aria-label="connection">
      <label htmlFor="aj-host">host</label>
      <input
        id="aj-host"
        value={host}
        onChange={(e) => setHost(e.target.value)}
        placeholder="127.0.0.1"
      />
      <label htmlFor="aj-port">port</label>
      <input
        id="aj-port"
        value={port}
        onChange={(e) => setPort(e.target.value)}
        placeholder="port"
      />
      <label htmlFor="aj-token">token</label>
      <input
        id="aj-token"
        value={token}
        onChange={(e) => setToken(e.target.value)}
        placeholder="token"
      />
      <button type="button" onClick={() => onConnect(host, port, token)}>
        Connect
      </button>
    </section>
  );
}
