#!/bin/bash
# Test script for Praxis + LiteLLM setup
#
# NOTE: The API key should be set in Praxis's environment when starting the server.
# Praxis will inject the Authorization header automatically via credential_injection filter.

set -e

# Send status messages to stderr so stdout is pure JSON
echo "Testing Praxis with LiteLLM endpoint..." >&2
echo "Sending test request to Praxis at http://localhost:8080/v1/chat/completions" >&2
echo "(Praxis will inject Authorization header from its environment and forward to LiteLLM)" >&2
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
