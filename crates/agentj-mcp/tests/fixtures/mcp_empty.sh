#!/bin/bash
# MCP server that returns an empty content array on tools/call.
read line
echo '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":"2024-11-05","serverInfo":{"name":"test","version":"1.0"},"capabilities":{}}}'
read line
echo '{"jsonrpc":"2.0","id":1,"result":{"tools":[{"name":"echo","description":"Echo input","inputSchema":{"type":"object","properties":{"text":{"type":"string"}}}}]}}'
read line
echo '{"jsonrpc":"2.0","id":2,"result":{"content":[]}}'
