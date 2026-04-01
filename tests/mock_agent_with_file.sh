#!/bin/bash
# Like mock_agent.sh but also creates an uncommitted file in the working directory.
# The executor should auto-commit this file after completion.
echo "auto-commit-test" > uncommitted_file.txt
prompt="${@: -1}"
escaped=$(printf '%s' "$prompt" | sed 's/\\/\\\\/g; s/"/\\"/g; s/	/\\t/g' | tr '\n' ' ')
echo "{\"type\": \"system\", \"subtype\": \"init\", \"session_id\": \"mock-session\", \"model\": \"mock-model\", \"tools\": []}"
echo "{\"type\": \"assistant\", \"message\": {\"content\": [{\"type\": \"text\", \"text\": \"mock: $escaped\"}]}, \"session_id\": \"mock-session\"}"
echo "{\"type\": \"result\", \"subtype\": \"success\", \"is_error\": false, \"result\": \"mock: $escaped\", \"session_id\": \"mock-session\", \"total_cost_usd\": 0.01, \"model\": \"mock-model\", \"usage\": {\"input_tokens\": 100, \"output_tokens\": 50}}"
