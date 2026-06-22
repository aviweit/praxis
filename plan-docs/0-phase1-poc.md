# Phase 1 POC: Minimal OpenAI Proxy

## Goal
Create a minimal proof-of-concept to validate that Praxis can:
1. Extract custom headers
2. Parse OpenAI request format
3. Forward to backend
4. Return response

**Time**: 1-2 hours  
**Scope**: Bare minimum to prove feasibility

---

## POC Architecture

```
Client → Praxis (header_manipulation + router + load_balancer) → Mock Backend
```

**Use existing filters only** - no custom code yet!

---

## POC Configuration

### File: `examples/configs/poc-openai-proxy.yaml`

```yaml
# POC: Minimal OpenAI Proxy using existing Praxis filters
# Tests: Can we proxy OpenAI requests with context extraction?

listeners:
  - name: poc-gateway
    address: "127.0.0.1:8080"
    filter_chains: [poc-chain]

filter_chains:
  - name: poc-chain
    filters:
      # Extract context header using existing header manipulation
      - filter: header_manipulation
        add:
          - name: "x-poc-env-id"
            value: "default"
        # Note: This is a workaround - we'll need custom filter for real extraction

      # Route to backend
      - filter: router
        routes:
          - path_prefix: "/v1/chat/completions"
            cluster: "mock-openai"
          - path_prefix: "/health"
            cluster: "local-health"

      # Load balance
      - filter: load_balancer
        clusters:
          - name: "mock-openai"
            endpoints:
              - "127.0.0.1:9000"  # Mock backend
          - name: "local-health"
            endpoints:
              - "127.0.0.1:8080"
```

---

## Mock Backend

Simple Python server to simulate OpenAI API:

### File: `examples/poc-mock-openai.py`

```python
#!/usr/bin/env python3
"""
Minimal mock OpenAI API for POC testing.
Usage: python3 examples/poc-mock-openai.py
"""

from http.server import HTTPServer, BaseHTTPRequestHandler
import json
import time

class MockOpenAIHandler(BaseHTTPRequestHandler):
    def do_POST(self):
        if self.path == '/v1/chat/completions':
            # Read request
            content_length = int(self.headers.get('Content-Length', 0))
            body = self.rfile.read(content_length)
            
            try:
                request = json.loads(body)
                print(f"[MOCK] Received request: model={request.get('model')}, "
                      f"messages={len(request.get('messages', []))}")
                
                # Build mock response
                response = {
                    "id": "chatcmpl-poc123",
                    "object": "chat.completion",
                    "created": int(time.time()),
                    "model": request.get("model", "gpt-4"),
                    "choices": [{
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": "POC response: Hello from mock OpenAI!"
                        },
                        "finish_reason": "stop"
                    }],
                    "usage": {
                        "prompt_tokens": 10,
                        "completion_tokens": 8,
                        "total_tokens": 18
                    }
                }
                
                # Send response
                self.send_response(200)
                self.send_header('Content-Type', 'application/json')
                self.end_headers()
                self.wfile.write(json.dumps(response).encode())
                
            except Exception as e:
                print(f"[MOCK] Error: {e}")
                self.send_response(500)
                self.end_headers()
        else:
            self.send_response(404)
            self.end_headers()
    
    def log_message(self, format, *args):
        # Suppress default logging
        pass

if __name__ == '__main__':
    server = HTTPServer(('127.0.0.1', 9000), MockOpenAIHandler)
    print("[MOCK] OpenAI server running on http://127.0.0.1:9000")
    print("[MOCK] Press Ctrl+C to stop")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\n[MOCK] Shutting down...")
        server.shutdown()
```

---

## Test Script

### File: `examples/test-poc.sh`

```bash
#!/bin/bash
# Test POC OpenAI Proxy

set -e

echo "=== Phase 1 POC Test ==="
echo

# Test 1: Basic chat completion
echo "Test 1: Basic chat completion request"
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "skillberry-context-env-id: poc-test" \
  -d '{
    "model": "gpt-4",
    "messages": [
      {"role": "user", "content": "Hello!"}
    ]
  }' | jq .

echo
echo "---"
echo

# Test 2: With temperature
echo "Test 2: Request with temperature parameter"
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4",
    "messages": [
      {"role": "system", "content": "You are helpful."},
      {"role": "user", "content": "Hi"}
    ],
    "temperature": 0.7,
    "max_tokens": 100
  }' | jq .

echo
echo "---"
echo

# Test 3: Invalid request (missing model)
echo "Test 3: Invalid request (should fail gracefully)"
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "messages": [
      {"role": "user", "content": "Hello"}
    ]
  }' || echo "Expected failure"

echo
echo "=== POC Tests Complete ==="
```

---

## Running the POC

### Terminal 1: Start Mock Backend
```bash
cd examples
python3 poc-mock-openai.py
```

### Terminal 2: Start Praxis
```bash
cargo run -- -c examples/configs/poc-openai-proxy.yaml
```

### Terminal 3: Run Tests
```bash
chmod +x examples/test-poc.sh
./examples/test-poc.sh
```

---

## Expected Results

### Success Criteria
- ✅ Praxis starts without errors
- ✅ Mock backend receives requests
- ✅ Responses return to client
- ✅ JSON format preserved
- ✅ Headers passed through

### What We Learn
1. **Routing works**: Praxis can route `/v1/chat/completions`
2. **Body passthrough**: Request/response bodies work
3. **JSON handling**: No corruption of JSON data
4. **Performance baseline**: Measure latency overhead

### What's Missing (for Phase 1)
- ❌ Custom header extraction (using workaround)
- ❌ Request validation (backend handles it)
- ❌ Response validation
- ❌ Metadata extraction
- ❌ Error handling

---

## POC Validation Checklist

- [ ] Mock backend starts on port 9000
- [ ] Praxis starts on port 8080
- [ ] Test 1 returns valid OpenAI response
- [ ] Test 2 handles parameters correctly
- [ ] Test 3 shows error handling (backend rejects)
- [ ] Latency overhead measured (<50ms target)
- [ ] No crashes or panics
- [ ] Logs show request flow

---

## Next Steps After POC

If POC succeeds:
1. ✅ Validates approach is feasible
2. ✅ Confirms Praxis can handle OpenAI format
3. ✅ Establishes performance baseline
4. → Proceed with custom filter implementation

If POC fails:
1. Identify blockers
2. Adjust approach
3. Re-evaluate architecture

---

## POC Success = Green Light for Phase 1

Once POC works, we have confidence to build:
- `context_extractor` (replaces header_manipulation workaround)
- `openai_request_parser` (adds validation)
- `openai_response_builder` (adds response handling)