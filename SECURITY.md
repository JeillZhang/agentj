# Security Policy

## Supported versions

AgentJ is pre-1.0 and ships from `main`. Security fixes land on `main` and are
released with the next patch version; there are no long-lived maintenance
branches yet.

| Version | Supported |
| ------- | --------- |
| 0.1.x   | ✅        |
| < 0.1   | ❌        |

## Reporting a vulnerability

Please **do not** open a public issue for a suspected vulnerability.

Report it privately through GitHub Security Advisories:

<https://github.com/JeillZhang/agentj/security/advisories/new>

Include, as far as you can:

- affected version or commit (`agentj --version`, `git rev-parse HEAD`)
- which component is involved (CLI / IPC gateway / Web server / TUI / Desktop)
- reproduction steps or a proof of concept
- impact assessment — what an attacker gains, and what access they need

You can expect an initial acknowledgement within 7 days. Once a fix is ready we
will coordinate a disclosure date with you and credit you in the advisory unless
you prefer to stay anonymous.

## Scope

In scope: the IPC gateway (`crates/agentj-ipc`, including the Web static server
and authentication), permission and question handling, credential storage
(`crates/agentj-auth`), the tool sandbox (`crates/agentj-tools`), and the
Desktop/TUI/Web clients.

Out of scope: vulnerabilities that require an already-compromised machine or a
malicious local `agentj.toml` the user was tricked into running, and issues in
third-party model providers.

## Handling secrets

AgentJ stores provider credentials in `auth.json` under the XDG data directory
with `0600` permissions. Never paste real API keys into issues, pull requests or
advisories; use redacted placeholders.
