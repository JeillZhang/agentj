#!/bin/bash
# mcp_lingering.sh — MCP fixture that completes the initialize/tools-list
# handshake, records its PID, then IGNORES stdin EOF and stays alive (BUG-001:
# an orphaned MCP subprocess survives the parent agentj process exiting without
# close_all). It still honors a graceful "shutdown" request so close_all can
# terminate it promptly instead of waiting for the 30s request timeout.
if [ -n "$PID_FILE" ]; then echo "$$" > "$PID_FILE"; fi
if [ -n "$MARKER" ]; then touch "$MARKER"; fi
read line
echo '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":"2024-11-05","serverInfo":{"name":"lingering","version":"1.0"},"capabilities":{}}}'
read line
echo '{"jsonrpc":"2.0","id":1,"result":{"tools":[{"name":"echo","description":"Echo input","inputSchema":{"type":"object","properties":{"text":{"type":"string"}}}}]}}'
# Respond to a graceful shutdown, but ignore stdin EOF and keep living otherwise.
while read line; do
  case "$line" in
    *'"method":"shutdown"'*) echo '{"jsonrpc":"2.0","id":2,"result":null}'; exit 0 ;;
  esac
done
# stdin EOF falls through here: spin forever to emulate an MCP server that
# ignores EOF, so a leaked subprocess is observable via its PID file.
while true; do sleep 3600; done