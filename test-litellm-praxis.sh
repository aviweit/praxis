#!/bin/bash
# Test script for Praxis + LiteLLM setup

set -e

# Check if OPENAI_API_KEY is set
if [ -z "$OPENAI_API_KEY" ]; then
    echo "ERROR: OPENAI_API_KEY environment variable is not set" >&2
    echo "Please run: export OPENAI_API_KEY=your-api-key" >&2
    exit 1
fi

# Send status messages to stderr so stdout is pure JSON
echo "Testing Praxis with LiteLLM endpoint..." >&2
echo "✓ OPENAI_API_KEY is set" >&2
echo "Sending test request to Praxis at http://localhost:8080/v1/chat/completions" >&2
echo "(Praxis will inject Authorization header and forward to LiteLLM)" >&2
echo "" >&2

curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "rits/openai/gpt-oss-120b",
    "messages": [
      {
        "role": "user",
        "content": "Hello! Tell me a happy joke."
      }
    ],
    "max_tokens": 500
  }'

# Send closing message to stderr
echo "" >&2
echo "Check the Praxis console output for the printed human message." >&2

# Made with Bob
