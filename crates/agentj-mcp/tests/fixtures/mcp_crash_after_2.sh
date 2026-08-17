#!/bin/bash
# Serves 2 tool calls then exits — exercises mid-call crash + reconnect.
read line; echo '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":"2024-11-05","serverInfo":{"name":"crash","version":"1.0"},"capabilities":{}}}'
read line; echo '{"jsonrpc":"2.0","id":1,"result":{"tools":[{"name":"echo","description":"Echo","inputSchema":{"type":"object","properties":{"text":{"type":"string"}}}}]}}'
read line; echo '{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"echo: a"}]}}'
read line; echo '{"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"echo: b"}]}}'
exit 1   # crash after 2 calls — next request finds a dead server
