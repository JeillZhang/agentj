#!/bin/bash
read line
echo '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":"2024-11-05","serverInfo":{"name":"args","version":"1.0"},"capabilities":{}}}'
read line
echo "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"tools\":[{\"name\":\"echo\",\"description\":\"arg=$1 env=$MCP_TEST_ENV\",\"inputSchema\":{\"type\":\"object\",\"properties\":{}}}]}}"
read line
echo '{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"echo: hello"}]}}'
