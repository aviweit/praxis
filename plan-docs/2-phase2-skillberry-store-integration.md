# Phase 2: Skillberry Store Integration & LLM Proxy

**Status**: Implemented
**Dependencies**: Phase 1 (context_extractor filter)

## Overview

Phase 2 integrates Praxis with Skillberry Store and LiteLLM to create a complete AI agent proxy:
1. Resolve skill UUIDs from environment variables or skill names
2. Create/manage Virtual MCP (VMCP) servers
3. Pass context (env_id) to VMCP servers
4. Proxy requests to LLM backend (via LiteLLM) with credential injection

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                     Incoming Request                             │
│         /v1/chat/completions + Authorization Header              │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│  context_extractor Filter                                        │
│  - Extracts env_id from x-skillberry-env-id header             │
│  - Stores in ctx.filter_metadata["env_id"]                      │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│  skill_resolver Filter                                           │
│  1. Read SKILL_UUID or SKILL_NAME from env vars                 │
│  2. If SKILL_NAME: HTTP GET to skillberry-store                 │
│     GET /skills/{skill_name} → returns skill with UUID          │
│  3. Store skill_uuid in ctx.filter_metadata["skill_uuid"]       │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│  vmcp_manager Filter                                             │
│  1. Get env_id from ctx.filter_metadata                         │
│  2. Get skill_uuid from ctx.filter_metadata (optional)          │
│  3. HTTP POST to skillberry-store to create VMCP server:        │
│     POST /vmcp_servers/ (query params: name, description, skill_uuid) │
│     Headers: skillberry-context-env-id: {env_id}                │
│  4. Store vmcp_port in ctx.filter_metadata["vmcp_port"]         │
│  5. Store vmcp_uuid in ctx.filter_metadata["vmcp_uuid"]         │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│  router + load_balancer                                         │
│  - Routes all paths (/) to llm_backend cluster                  │
│  - Forwards request as-is to LiteLLM backend                    │
│  - Client Authorization header passed through                    │
│  - Backend: localhost:4000 (configurable)                       │
└─────────────────────────────────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                    LiteLLM Backend                               │
│  - Receives request with Authorization header                   │
│  - Routes to appropriate LLM provider based on model name       │
│  - Returns response to Praxis                                   │
└─────────────────────────────────────────────────────────────────┘
```

**Key Changes from Original Design**:
- **No credential_injection filter**: Clients send their own `Authorization: Bearer` header
- **Simplified routing**: Single route matches all paths (`/`)
- **Pass-through proxy**: Request forwarded as-is to LiteLLM backend
- **Client-side authentication**: API key management handled by client

## LLM Backend Integration

Phase 2 uses **LiteLLM** as the LLM backend, which provides:
- Unified OpenAI-compatible API for 100+ LLM providers
- Support for OpenAI, Anthropic, Azure, AWS Bedrock, Google, etc.
- Automatic retry logic and fallback handling
- Cost tracking and usage analytics

### Environment Variables

```bash
# LLM Configuration
export OPENAI_BASE_URL="http://localhost:4000"  # LiteLLM proxy endpoint
export OPENAI_API_KEY="your-api-key"            # Your LLM provider API key

# Skillberry Store Configuration
export SKILL_UUID="869ff6a0-acb4-4759-a065-4cb2ae43a5f3"  # OR
export SKILL_NAME="flight_reservation_management"
```

## Usage

### Python Client with LiteLLM Package

```python
#!/usr/bin/env python3
"""
Test Praxis Phase 2 with LiteLLM package
Sends request through Praxis proxy to LiteLLM backend
"""
import os
from litellm import completion

# Configure to use Praxis proxy
os.environ["OPENAI_API_BASE"] = "http://localhost:8080/v1"
os.environ["OPENAI_API_KEY"] = "your-api-key-here"

# Make request through Praxis
response = completion(
    model="openai/rits/openai/gpt-oss-120b",
    messages=[
        {"role": "user", "content": "Hello, how are you?"}
    ],
    extra_headers={
        "x-skillberry-env-id": "test-env-123"
    }
)

print(response.choices[0].message.content)
```

**Installation**:
```bash
pip install litellm
```

**Run**:
```bash
python test_praxis_phase2.py
```


## Skillberry Store HTTP API

Based on the Python SDK analysis, the store exposes these endpoints:

### 1. Get Skill by Name or UUID
```
GET /skills/{uuid_or_name}
Response: {
  "uuid": "550e8400-e29b-41d4-a716-446655440000",
  "name": "airline-booking",
  "description": "...",
  "tools": [...],
  ...
}
```

### 2. Create VMCP Server
```
POST /vmcp-servers
Headers:
  skillberry-context-env-id: {env_id}
Body: {
  "name": "vmcp-{env_id}",
  "skill_uuid": "550e8400-e29b-41d4-a716-446655440000",  // optional
  "description": "VMCP server for env {env_id}"
}
Response: {
  "uuid": "vmcp-uuid-here",
  "name": "vmcp-{env_id}",
  "port": 8001,
  "skill_uuid": "...",
  "runtime_tools": [...],
  ...
}
```

### 3. Get VMCP Server Details
```
GET /vmcp-servers/{uuid_or_name}
Response: {
  "uuid": "vmcp-uuid-here",
  "name": "vmcp-{env_id}",
  "port": 8001,
  "skill_uuid": "...",
  "runtime_tools": [...],
  ...
}
```

### 4. Delete VMCP Server
```
DELETE /vmcp-servers/{uuid_or_name}
Response: {"message": "success"}
```

## New Filters to Implement

### Filter 1: `skill_resolver`

**Purpose**: Resolve skill UUID from environment variables

**Configuration**:
```yaml
filter: skill_resolver
store_base_url: "http://localhost:8000"  # skillberry-store URL
skill_uuid_env: "SKILL_UUID"              # env var for direct UUID
skill_name_env: "SKILL_NAME"              # env var for skill name lookup
timeout_ms: 5000                          # HTTP request timeout
```

**Logic**:
1. Check `SKILL_UUID` env var → if set, use directly
2. Else check `SKILL_NAME` env var → if set, lookup via HTTP
3. Store result in `ctx.filter_metadata["skill_uuid"]`
4. If neither set, continue without skill (VMCP will be created without skill)

**HTTP Request** (if SKILL_NAME is set):
```
GET {store_base_url}/skills/{skill_name}
Accept: application/json
```

**Error Handling**:
- If skill name not found (404): Log warning, continue without skill
- If store unreachable: Return 503 Service Unavailable
- If timeout: Return 504 Gateway Timeout

**Metadata Output**:
- `skill_uuid`: The resolved UUID (or empty if not resolved)
- `skill_name`: The skill name (if resolved via name lookup)

---

### Filter 2: `vmcp_manager`

**Purpose**: Create/manage VMCP server for the request

**Configuration**:
```yaml
filter: vmcp_manager
store_base_url: "http://localhost:8000"
vmcp_name_template: "vmcp-{env_id}"      # Template for VMCP server name
always_create: true                       # Phase 2: always create new
reuse_existing: false                     # Phase 3: reuse if exists
timeout_ms: 10000                         # HTTP request timeout
cleanup_on_error: true                    # Delete VMCP if request fails
```

**Logic**:
1. Get `env_id` from `ctx.filter_metadata["env_id"]` (required)
2. Get `skill_uuid` from `ctx.filter_metadata["skill_uuid"]` (optional)
3. Generate VMCP name: `vmcp-{env_id}`
4. HTTP POST to create VMCP server
5. Store VMCP details in metadata

**HTTP Request**:
```
POST {store_base_url}/vmcp-servers
Headers:
  Content-Type: application/json
  skillberry-context-env-id: {env_id}
Body:
{
  "name": "vmcp-{env_id}",
  "skill_uuid": "{skill_uuid}",  // optional, omit if not resolved
  "description": "VMCP server for environment {env_id}"
}
```

**Response Handling**:
```json
{
  "uuid": "vmcp-abc123",
  "name": "vmcp-prod-env",
  "port": 8001,
  "skill_uuid": "skill-uuid-here",
  "runtime_tools": [
    {"name": "search_flights", ...},
    {"name": "book_flight", ...}
  ]
}
```

**Metadata Output**:
- `vmcp_uuid`: UUID of created VMCP server
- `vmcp_name`: Name of VMCP server
- `vmcp_port`: Port where VMCP server is listening
- `vmcp_tools_count`: Number of tools available

**Error Handling**:
- If env_id missing: Return 400 Bad Request
- If store unreachable: Return 503 Service Unavailable
- If VMCP creation fails: Return 500 Internal Server Error
- If 409 Conflict (already exists): 
  - Phase 2: Return error (always_create=true)
  - Phase 3: Reuse existing (reuse_existing=true)

**Cleanup** (Phase 3):
- On request completion: Keep VMCP running (for reuse)
- On error: Delete VMCP if `cleanup_on_error=true`

---

## Implementation Details

### Filter Structure

Both filters follow the standard Praxis filter pattern:

```
filter/src/builtins/http/skillberry/
├── skill_resolver/
│   ├── mod.rs
│   ├── config.rs
│   ├── filter.rs
│   └── tests.rs
└── vmcp_manager/
    ├── mod.rs
    ├── config.rs
    ├── filter.rs
    └── tests.rs
```

### HTTP Client

Use `reqwest` for HTTP requests:

```rust
use reqwest::Client;
use serde::{Deserialize, Serialize};

// In filter struct
pub struct SkillResolverFilter {
    http_client: Client,
    store_base_url: String,
    skill_uuid_env: String,
    skill_name_env: String,
    timeout: Duration,
}

// HTTP request
async fn get_skill(&self, skill_name: &str) -> Result<SkillResponse, FilterError> {
    let url = format!("{}/skills/{}", self.store_base_url, skill_name);
    
    let response = self.http_client
        .get(&url)
        .timeout(self.timeout)
        .send()
        .await
        .map_err(|e| FilterError::from(format!("HTTP request failed: {}", e)))?;
    
    if response.status().is_success() {
        response.json().await
            .map_err(|e| FilterError::from(format!("JSON parse failed: {}", e)))
    } else {
        Err(FilterError::from(format!("HTTP {} error", response.status())))
    }
}
```

### Response Types

```rust
#[derive(Debug, Deserialize)]
struct SkillResponse {
    uuid: String,
    name: String,
    description: Option<String>,
    // ... other fields
}

#[derive(Debug, Serialize)]
struct CreateVmcpRequest {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    skill_uuid: Option<String>,
    description: String,
}

#[derive(Debug, Deserialize)]
struct VmcpResponse {
    uuid: String,
    name: String,
    port: u16,
    skill_uuid: Option<String>,
    runtime_tools: Vec<serde_json::Value>,
}
```

### Environment Variable Access

```rust
use std::env;

fn get_skill_uuid_from_env(&self) -> Option<String> {
    env::var(&self.skill_uuid_env).ok()
}

fn get_skill_name_from_env(&self) -> Option<String> {
    env::var(&self.skill_name_env).ok()
}
```

### Context Header Injection

```rust
// In vmcp_manager filter
let env_id = ctx.filter_metadata
    .get("env_id")
    .ok_or_else(|| FilterError::bad_request("env_id not found in metadata"))?;

let mut headers = HeaderMap::new();
headers.insert(
    "skillberry-context-env-id",
    HeaderValue::from_str(env_id)
        .map_err(|e| FilterError::from(format!("Invalid env_id: {}", e)))?
);

let response = self.http_client
    .post(&url)
    .headers(headers)
    .json(&request_body)
    .send()
    .await?;
```

## Configuration Example

```yaml
listeners:
  - name: skillberry-gateway
    address: "0.0.0.0:8080"
    filter_chains:
      - context-extraction
      - skill-resolution
      - vmcp-management
      - routing

filter_chains:
  # Phase 1: Extract context from headers
  - name: context-extraction
    filters:
      - filter: context_extractor
        headers:
          - name: skillberry-context-env-id
            metadata_key: env_id
            default: "default-env"
            required: true

  # Phase 2: Resolve skill UUID
  - name: skill-resolution
    filters:
      - filter: skill_resolver
        store_base_url: "http://localhost:8000"
        skill_uuid_env: "SKILL_UUID"
        skill_name_env: "SKILL_NAME"
        timeout_ms: 5000

  # Phase 2: Create VMCP server
  - name: vmcp-management
    filters:
      - filter: vmcp_manager
        store_base_url: "http://localhost:8000"
        vmcp_name_template: "vmcp-{env_id}"
        always_create: true
        timeout_ms: 10000

  # Route to OpenAI/LiteLLM
  - name: routing
    filters:
      - filter: router
        routes:
          - path_prefix: "/v1/"
            cluster: openai
      - filter: load_balancer
        clusters:
          - name: openai
            endpoints:
              - "localhost:4000"
```

## Testing Strategy

### Unit Tests

**skill_resolver**:
- ✅ Direct UUID from env var
- ✅ Skill name lookup (success)
- ✅ Skill name lookup (404 not found)
- ✅ Store unreachable (connection error)
- ✅ Timeout handling
- ✅ Invalid JSON response
- ✅ Neither UUID nor name set (continue without skill)

**vmcp_manager**:
- ✅ Create VMCP with skill UUID
- ✅ Create VMCP without skill UUID
- ✅ Missing env_id (error)
- ✅ Store unreachable (503 error)
- ✅ VMCP creation timeout
- ✅ Invalid response format
- ✅ 409 Conflict handling

### Integration Tests

1. **End-to-end with mock store**:
   - Mock skillberry-store HTTP server
   - Test full request flow
   - Verify metadata propagation

2. **Real store integration**:
   - Test with actual skillberry-store instance
   - Verify VMCP server creation
   - Check VMCP server cleanup

3. **Error scenarios**:
   - Store down during request
   - Skill not found
   - VMCP creation failure

## Performance Considerations

### Latency Budget

- Skill resolution: < 50ms (cached in Phase 3)
- VMCP creation: < 200ms (one-time per env_id)
- Total Phase 2 overhead: < 250ms

### Optimization Opportunities (Phase 3)

1. **Skill UUID Caching**:
   - Cache skill name → UUID mappings
   - TTL: 5 minutes
   - Reduces store lookups

2. **VMCP Reuse**:
   - Keep VMCP servers alive
   - Reuse by env_id
   - Reduces creation overhead

3. **Connection Pooling**:
   - Reuse HTTP connections to store
   - Reduces connection overhead

## Phase 2 Deliverables

### Week 1: skill_resolver Filter
- [ ] Implement config.rs
- [ ] Implement filter.rs with HTTP client
- [ ] Add environment variable reading
- [ ] Write unit tests
- [ ] Register in filter registry
- [ ] Integration test with mock store

### Week 2: vmcp_manager Filter
- [ ] Implement config.rs
- [ ] Implement filter.rs with HTTP client
- [ ] Add context header injection
- [ ] Write unit tests
- [ ] Register in filter registry
- [ ] Integration test with mock store

### Week 3: Integration & Testing
- [ ] End-to-end testing with real store
- [ ] Performance benchmarking
- [ ] Error handling validation
- [ ] Documentation
- [ ] Example configurations

## Dependencies

### Rust Crates

Add to `filter/Cargo.toml`:
```toml
[dependencies]
reqwest = { version = "0.11", features = ["json"] }
tokio = { version = "1", features = ["time"] }
```

### Environment Variables

Required at runtime:
- `SKILL_UUID` or `SKILL_NAME` (one of them, optional)
- Skillberry store must be running at configured URL

## Migration from Python

### Python (skillberry-agent)
```python
# skill_manager.py
skill = skillberry_store.get_skill(skill_name)
skill_uuid = skill.get('uuid')

# vmcp_server_manager.py
vmcp_response = skillberry_store.add_vmcp_server(
    name=server_name,
    skill_uuid=skill_uuid,
    description=description,
    skillberry_context={"env_id": env_id}
)
```

### Rust (Praxis)
```rust
// skill_resolver filter
let skill = self.get_skill(skill_name).await?;
ctx.filter_metadata.insert("skill_uuid".to_string(), skill.uuid);

// vmcp_manager filter
let vmcp = self.create_vmcp_server(
    name,
    skill_uuid,
    env_id
).await?;
ctx.filter_metadata.insert("vmcp_port".to_string(), vmcp.port.to_string());
```

## Next Steps (Phase 3)

After Phase 2 completion:
1. Implement MCP tool invocation filter
2. Add VMCP server reuse logic
3. Implement caching for skill lookups
4. Add VMCP lifecycle management (cleanup)
5. Integrate with agentic workflow

## Open Questions

1. **VMCP Lifecycle**: When to delete VMCP servers?
   - Option A: Delete after each request (Phase 2)
   - Option B: Keep alive and reuse (Phase 3)
   - **Decision**: Phase 2 always creates, Phase 3 adds reuse

2. **Error Recovery**: What if VMCP creation fails mid-request?
   - **Decision**: Return 500, optionally cleanup partial state

3. **Concurrent Requests**: Multiple requests with same env_id?
   - **Decision**: Phase 2 creates separate VMCPs, Phase 3 adds locking

4. **Store Authentication**: Does skillberry-store require auth?
   - **Decision**: TBD based on store deployment

## Success Criteria

Phase 2 is complete when:
- ✅ skill_resolver filter resolves UUIDs from env vars
- ✅ vmcp_manager filter creates VMCP servers
- ✅ Context (env_id) is passed to VMCP servers
- ✅ All unit tests pass
- ✅ Integration tests with real store pass
- ✅ Performance overhead < 250ms
- ✅ Documentation complete
- ✅ Example configurations work

---

**Created**: 2026-06-21  
**Author**: Bob (AI Assistant)  
**Status**: Design Complete, Ready for Implementation