# Phase 2: Skillberry Store Integration - Testing Guide

## Overview

This guide provides instructions for testing the Phase 2 implementation of Praxis integration with skillberry-store. The implementation includes two new filters:

1. **skill_resolver**: Resolves skill UUIDs from environment variables via skillberry-store API
2. **vmcp_manager**: Creates VMCP servers with context propagation

## Prerequisites

### 1. Build Praxis

```bash
cd /path/to/praxis
cargo build --release
```

### 2. Start skillberry-store

Ensure skillberry-store is running and accessible:

```bash
# Check if skillberry-store is running
curl http://localhost:8000/health

# Expected response: {"status": "healthy"}
```

### 3. Set Environment Variables

```bash
# Option 1: Use skill UUID directly
export SKILL_UUID="550e8400-e29b-41d4-a716-446655440000"

# Option 2: Use skill name (will be resolved to UUID)
export SKILL_NAME="my-test-skill"

# Context identifier (optional, has default)
export SKILLBERRY_CONTEXT_ENV_ID="test-env-123"
```

## Test Scenarios

### Scenario 1: Basic Filter Chain Test

Test all three filters working together with valid inputs.

**Configuration**: `examples/configs/ai/phase2-skillberry-integration.yaml`

**Start Praxis**:
```bash
cd /path/to/praxis
./target/release/praxis -c examples/configs/ai/phase2-skillberry-integration.yaml
```

**Test Request**:
```bash
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "x-skillberry-env-id: test-env-123" \
  -H "x-skillberry-user-id: user-456" \
  -H "x-skillberry-session-id: session-789" \
  -d '{
    "model": "gpt-4",
    "messages": [
      {"role": "user", "content": "Hello, world!"}
    ]
  }'
```

**Expected Behavior**:
1. context_extractor extracts headers to metadata
2. skill_resolver calls GET /skills/{uuid_or_name} on skillberry-store
3. vmcp_manager calls POST /vmcp-servers with context headers
4. Request proxied to OpenAI backend (LiteLLM)
5. Response returned to client

**Verify**:
- Check Praxis logs for filter execution
- Check skillberry-store logs for API calls
- Verify VMCP server was created in skillberry-store

### Scenario 2: Missing Skill (Optional Mode)

Test behavior when skill is not found but filter is configured as optional.

**Setup**:
```bash
# Set non-existent skill
export SKILL_NAME="non-existent-skill"
```

**Test Request**: Same as Scenario 1

**Expected Behavior**:
1. skill_resolver attempts to resolve skill
2. GET /skills/non-existent-skill returns 404
3. Filter logs warning but continues (optional: true)
4. vmcp_manager creates VMCP without skill_uuid
5. Request continues to backend

**Verify**:
- Check logs for "Skill not found" warning
- Verify request still succeeds
- Check VMCP server created without skill_uuid

### Scenario 3: Skill Resolution by Name

Test skill name resolution to UUID.

**Setup**:
```bash
# First, create a skill in skillberry-store
curl -X POST http://localhost:8000/skills \
  -H "Content-Type: application/json" \
  -d '{
    "name": "test-skill-phase2",
    "description": "Test skill for Phase 2",
    "tools": []
  }'

# Set skill name
export SKILL_NAME="test-skill-phase2"
unset SKILL_UUID
```

**Test Request**: Same as Scenario 1

**Expected Behavior**:
1. skill_resolver reads SKILL_NAME from env
2. GET /skills/test-skill-phase2 returns skill with UUID
3. UUID stored in metadata
4. vmcp_manager uses resolved UUID

**Verify**:
- Check logs for skill resolution
- Verify correct UUID used in VMCP creation

### Scenario 4: Context Header Validation

Test header validation rules.

**Test Invalid env_id**:
```bash
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "x-skillberry-env-id: invalid@env#id!" \
  -d '{"model": "gpt-4", "messages": [{"role": "user", "content": "test"}]}'
```

**Expected**: 400 Bad Request (validation fails)

**Test Missing env_id (uses default)**:
```bash
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model": "gpt-4", "messages": [{"role": "user", "content": "test"}]}'
```

**Expected**: Success with default env_id "default-env"

### Scenario 5: skillberry-store Unavailable

Test error handling when skillberry-store is down.

**Setup**:
```bash
# Stop skillberry-store temporarily
```

**Test Request**: Same as Scenario 1

**Expected Behavior**:
1. skill_resolver attempts connection
2. Connection fails or times out
3. Returns 503 Service Unavailable
4. Error logged with details

**Verify**:
- Check error response
- Verify appropriate error message
- Confirm request not proxied to backend

### Scenario 6: VMCP Creation Timeout

Test timeout handling for slow VMCP creation.

**Setup**: Configure shorter timeout in YAML:
```yaml
- type: vmcp_manager
  config:
    timeout_ms: 100  # Very short timeout
```

**Expected**: 504 Gateway Timeout if creation takes >100ms

### Scenario 7: Concurrent Requests

Test multiple concurrent requests to verify thread safety.

**Load Test**:
```bash
# Install hey if not available
go install github.com/rakyll/hey@latest

# Run load test
hey -n 100 -c 10 -m POST \
  -H "Content-Type: application/json" \
  -H "x-skillberry-env-id: load-test" \
  -d '{"model":"gpt-4","messages":[{"role":"user","content":"test"}]}' \
  http://localhost:8080/v1/chat/completions
```

**Expected**:
- All requests succeed
- No race conditions
- Consistent VMCP creation
- Performance within targets (<250ms overhead)

**Verify**:
- Check response times
- Verify all VMCP servers created
- Check for any errors in logs

## Debugging

### Enable Debug Logging

Add to configuration:
```yaml
runtime:
  log_level: debug
```

### Check Filter Metadata

Add access_log filter to see metadata:
```yaml
response_filters:
  - type: access_log
    config:
      format: "env_id={env_id} skill_uuid={skill_uuid} vmcp_url={vmcp_url}"
```

### Inspect skillberry-store State

```bash
# List all VMCP servers
curl http://localhost:8000/vmcp-servers

# Get specific VMCP server
curl http://localhost:8000/vmcp-servers/{vmcp_id}

# List all skills
curl http://localhost:8000/skills
```

### Common Issues

**Issue**: "Skill not found" but skill exists
- **Solution**: Check SKILL_NAME matches exactly (case-sensitive)
- **Solution**: Verify skill exists: `curl http://localhost:8000/skills/{name}`

**Issue**: VMCP creation fails
- **Solution**: Check skillberry-store logs for errors
- **Solution**: Verify context headers are valid
- **Solution**: Check skillberry-store has capacity

**Issue**: Timeout errors
- **Solution**: Increase timeout_ms in configuration
- **Solution**: Check network latency to skillberry-store
- **Solution**: Verify skillberry-store is responsive

**Issue**: Compilation errors
- **Solution**: Run `cargo clean && cargo build`
- **Solution**: Check all dependencies in Cargo.toml
- **Solution**: Verify reqwest feature flags

## Performance Benchmarks

### Target Metrics

- **Latency Overhead**: < 250ms per request
  - skill_resolver: < 50ms
  - vmcp_manager: < 200ms
- **Throughput**: > 100 req/s
- **Memory**: < 10MB per filter instance

### Benchmark Commands

```bash
# Measure skill_resolver latency
time curl http://localhost:8000/skills/test-skill

# Measure vmcp_manager latency
time curl -X POST http://localhost:8000/vmcp-servers \
  -H "Content-Type: application/json" \
  -d '{"name":"test","skill_uuid":"...","context":{}}'

# End-to-end latency
time curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4","messages":[{"role":"user","content":"test"}]}'
```

## Integration with skillberry-store API

### API Endpoints Used

**GET /skills/{uuid_or_name}**
- Used by: skill_resolver
- Purpose: Resolve skill name to UUID
- Response: `{"uuid": "...", "name": "...", ...}`

**POST /vmcp-servers**
- Used by: vmcp_manager
- Purpose: Create VMCP server
- Headers: `skillberry-context-env-id`, `skillberry-context-skill-uuid`
- Request: `{"name": "...", "skill_uuid": "...", "always_create": true}`
- Response: `{"id": "...", "url": "...", "status": "running"}`

### Expected skillberry-store Behavior

1. Skill resolution should be fast (< 50ms)
2. VMCP creation may take longer (< 200ms)
3. VMCP servers should be immediately available after creation
4. Context headers should be propagated to VMCP server

## Next Steps

After successful Phase 2 testing:

1. **Phase 3 Implementation**:
   - MCP tool invocation filter
   - VMCP reuse logic
   - Caching layer

2. **Performance Optimization**:
   - Connection pooling for skillberry-store
   - Async VMCP creation
   - Metadata caching

3. **Production Readiness**:
   - Error recovery strategies
   - Circuit breaker for skillberry-store
   - Monitoring and alerting
   - Load testing at scale

## Reporting Issues

When reporting issues, include:

1. Praxis version and commit hash
2. Configuration file used
3. Environment variables set
4. Full error logs from Praxis
5. skillberry-store logs (if relevant)
6. Request/response examples
7. Expected vs actual behavior

## Summary

Phase 2 implementation provides the foundation for Praxis-skillberry integration:

✅ **Implemented**:
- context_extractor filter (Phase 1)
- skill_resolver filter (Phase 2)
- vmcp_manager filter (Phase 2)
- HTTP client integration with reqwest
- Error handling and timeouts
- Example configuration

🔄 **In Progress**:
- Real-world testing with skillberry-store
- Performance validation
- Edge case handling

⏳ **Future** (Phase 3+):
- MCP tool invocation
- VMCP reuse and caching
- Agentic workflow integration
- OpenAI request/response handling