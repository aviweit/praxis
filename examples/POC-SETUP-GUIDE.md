# POC Setup Guide: Praxis + LiteLLM/OpenAI

This guide walks you through setting up and testing the Praxis POC as a proxy to LiteLLM or OpenAI.

## What This POC Does

1. **Receives** OpenAI-compatible `/v1/chat/completions` requests
2. **Logs** human messages to the console (via `print_human_message` filter)
3. **Injects** API credentials from environment variables (via `credential_injection` filter)
4. **Forwards** requests to your LiteLLM/OpenAI backend
5. **Returns** responses to the client

## Architecture

```
Client Request
    ↓
Praxis (localhost:8080)
    ↓
[print_human_message] → Logs user messages to console
    ↓
[router] → Routes to "openai" cluster
    ↓
[load_balancer] → Selects endpoint
    ↓
[credential_injection] → Adds "Authorization: Bearer $OPENAI_API_KEY"
    ↓
LiteLLM/OpenAI Endpoint
    ↓
Response back to client
```

## Prerequisites

1. **Rust toolchain** installed (for building Praxis)
2. **LiteLLM server** running OR **OpenAI API access**
3. **API key** for authentication
4. **jq** installed (for testing - optional but recommended)

## Setup Steps

### Step 1: Set Environment Variables

```bash
export OPENAI_API_KEY="your-api-key-here"
```

**Note:** The API key is read at Praxis startup and injected into requests automatically.

### Step 2: Configure Your Endpoint

Edit `examples/configs/poc-openai-proxy.yaml` and replace the placeholder endpoint:

```yaml
endpoints:
  - "REPLACE_WITH_YOUR_ENDPOINT"  # Change this line
```

**Endpoint Format:**
- Use `hostname:port` format (no `http://` prefix)
- Examples:
  - `localhost:4000` (local LiteLLM)
  - `my-litellm-server.com:4000` (remote LiteLLM)
  - `api.openai.com:443` (direct OpenAI - Praxis auto-enables TLS for port 443)

**Example for local LiteLLM:**
```yaml
endpoints:
  - "localhost:4000"
```

**Example for remote server:**
```yaml
endpoints:
  - "my-server.example.com:4000"
```

### Step 3: Build Praxis

```bash
cd /path/to/praxis
cargo build --release
```

This will create the binary at `target/release/praxis`.

### Step 4: Start Praxis

```bash
./target/release/praxis --config examples/configs/poc-openai-proxy.yaml
```

You should see output like:
```
[INFO] Starting Praxis proxy server
[INFO] Listening on 0.0.0.0:8080
[INFO] Credential injection configured for cluster 'openai'
```

### Step 5: Test the Proxy

In a new terminal, run the test script:

```bash
cd /path/to/praxis
chmod +x examples/test-poc.sh
./examples/test-poc.sh
```

Or test manually with curl:

```bash
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-3.5-turbo",
    "messages": [
      {"role": "user", "content": "Hello! Say hi in one word."}
    ],
    "max_tokens": 10
  }'
```

### Step 6: Verify Output

**In the Praxis console**, you should see the human message printed:

```
============================================================
Human Message from Chat Request
============================================================
Hello! Say hi in one word.
============================================================
```

**In the curl response**, you should see the OpenAI response:

```json
{
  "id": "chatcmpl-...",
  "object": "chat.completion",
  "created": 1234567890,
  "model": "gpt-3.5-turbo",
  "choices": [
    {
      "index": 0,
      "message": {
        "role": "assistant",
        "content": "Hi!"
      },
      "finish_reason": "stop"
    }
  ],
  "usage": {
    "prompt_tokens": 15,
    "completion_tokens": 2,
    "total_tokens": 17
  }
}
```

## Configuration Details

### Filter Chains

The POC uses three filter chains:

1. **observability**: Logs human messages
   ```yaml
   - filter: print_human_message
   ```

2. **routing**: Routes requests to backend
   ```yaml
   - filter: router
     routes:
       - path_prefix: "/v1/"
         cluster: openai
   - filter: load_balancer
     clusters:
       - name: openai
         endpoints:
           - "your-endpoint:port"
   ```

3. **credentials**: Injects API key
   ```yaml
   - filter: credential_injection
     clusters:
       - name: openai
         header: Authorization
         env_var: OPENAI_API_KEY
         header_prefix: "Bearer "
   ```

### Security Notes

- ✅ API key is read from environment variable (not hardcoded)
- ✅ Credentials are stored securely in memory (zeroized on drop)
- ✅ Client-provided Authorization headers are stripped
- ✅ Only Praxis-injected credentials are sent to backend

## Troubleshooting

### Error: "environment variable 'OPENAI_API_KEY' not set"

**Cause:** The environment variable is not set when Praxis starts.

**Solution:**
```bash
export OPENAI_API_KEY="your-key"
./target/release/praxis --config examples/configs/poc-openai-proxy.yaml
```

### Error: "invalid port value" or "failed to parse endpoint"

**Cause:** Incorrect endpoint format in YAML.

**Wrong:**
```yaml
endpoints:
  - "http://localhost:4000"  # ❌ Don't include http://
```

**Correct:**
```yaml
endpoints:
  - "localhost:4000"  # ✅ Just hostname:port
```

### Error: "connection refused"

**Cause:** LiteLLM/OpenAI endpoint is not reachable.

**Solutions:**
1. Verify LiteLLM is running: `curl http://localhost:4000/health`
2. Check firewall rules
3. Verify endpoint hostname and port in config

### No messages printed to console

**Cause:** Request body doesn't contain user messages or isn't valid JSON.

**Check:**
1. Request has `Content-Type: application/json` header
2. Body contains `messages` array
3. At least one message has `role: "user"`

### 401 Unauthorized from backend

**Cause:** Invalid or missing API key.

**Solutions:**
1. Verify `OPENAI_API_KEY` is set correctly
2. Check API key is valid for your LiteLLM/OpenAI account
3. Restart Praxis after changing environment variable

## Testing Different Scenarios

### Test 1: Basic Request
```bash
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-3.5-turbo",
    "messages": [{"role": "user", "content": "Hello"}]
  }'
```

### Test 2: Multi-turn Conversation
```bash
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-3.5-turbo",
    "messages": [
      {"role": "system", "content": "You are helpful."},
      {"role": "user", "content": "What is 2+2?"},
      {"role": "assistant", "content": "4"},
      {"role": "user", "content": "Thanks!"}
    ]
  }'
```

**Expected:** Only the two user messages are printed to console.

### Test 3: With Parameters
```bash
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-3.5-turbo",
    "messages": [{"role": "user", "content": "Count to 5"}],
    "temperature": 0.7,
    "max_tokens": 50
  }'
```

## Performance Expectations

This POC uses existing Praxis filters with minimal overhead:

- **Latency overhead**: < 5ms (just routing + credential injection)
- **Throughput**: Limited by backend, not Praxis
- **Memory**: Minimal (no body buffering except for message logging)

## Next Steps

After validating the POC works:

1. **Measure baseline performance** (see `plan-docs/0-phase1-poc.md`)
2. **Implement Phase 1 custom filters** (see `plan-docs/1-phase1-basic-openai-proxy.md`)
3. **Add context extraction** (headers → metadata)
4. **Add request validation** (OpenAI schema validation)
5. **Add response enrichment** (custom headers, logging)

## Files Reference

- **Config**: `examples/configs/poc-openai-proxy.yaml`
- **Test Script**: `examples/test-poc.sh`
- **Setup Guide**: `examples/POC-SETUP-GUIDE.md` (this file)
- **Phase 1 Plan**: `plan-docs/1-phase1-basic-openai-proxy.md`
- **Design Doc**: `plan-docs/skillberry-agent-to-praxis-mapping.md`

## Support

For issues or questions:
1. Check the troubleshooting section above
2. Review Praxis logs for error messages
3. Verify LiteLLM/OpenAI endpoint is accessible
4. Check environment variables are set correctly