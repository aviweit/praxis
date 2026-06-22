#!/bin/bash
# Test POC OpenAI Proxy with Real LiteLLM/OpenAI
#
# Prerequisites:
# 1. Set OPENAI_API_KEY environment variable
# 2. Edit examples/configs/poc-openai-proxy.yaml with your endpoint
# 3. Start Praxis: cargo run --release -- --config examples/configs/poc-openai-proxy.yaml

set -e

# Check if OPENAI_API_KEY is set
if [ -z "$OPENAI_API_KEY" ]; then
  echo "❌ Error: OPENAI_API_KEY environment variable is not set"
  echo "Please run: export OPENAI_API_KEY='your-api-key-here'"
  exit 1
fi

echo "=== POC OpenAI Proxy Test ==="
echo "Testing Praxis proxy to LiteLLM/OpenAI"
echo

# Test 1: Basic chat completion
echo "Test 1: Basic chat completion request"
echo "Expected: Human message printed to Praxis console, response from LLM"
curl -s -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-3.5-turbo",
    "messages": [
      {"role": "user", "content": "Say hello in one word"}
    ],
    "max_tokens": 10
  }' | jq .

echo
echo "---"
echo

# Test 2: Multi-turn conversation
echo "Test 2: Multi-turn conversation"
echo "Expected: Only user messages printed to console"
curl -s -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-3.5-turbo",
    "messages": [
      {"role": "system", "content": "You are a helpful assistant."},
      {"role": "user", "content": "What is 2+2?"},
      {"role": "assistant", "content": "4"},
      {"role": "user", "content": "Thanks!"}
    ],
    "max_tokens": 20
  }' | jq .

echo
echo "---"
echo

# Test 3: With temperature and max_tokens
echo "Test 3: Request with parameters"
curl -s -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-3.5-turbo",
    "messages": [
      {"role": "user", "content": "Count to 3"}
    ],
    "temperature": 0.7,
    "max_tokens": 20
  }' | jq .

echo
echo "---"
echo

# Test 4: Error handling - invalid request
echo "Test 4: Invalid request (missing model)"
echo "Expected: Error from backend"
curl -s -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "messages": [
      {"role": "user", "content": "Hello"}
    ]
  }' | jq . || echo "Backend rejected (expected)"

echo
echo "=== POC Tests Complete ==="
echo
echo "✅ Check Praxis console output for printed human messages"
echo "✅ All requests should have received responses from LiteLLM/OpenAI"

# Made with Bob
