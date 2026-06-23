# Phase 3: MCP Tools Integration & Request Enrichment

**Status**: ✅ Implemented
**Dependencies**: Phase 2 (vmcp_manager, skill_resolver)

## Overview

Phase 3 enriches the OpenAI chat completion request with MCP tools schema from the created VMCP server before forwarding to the LLM backend. This enables the LLM to understand and call the available MCP tools.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                     Incoming Request                             │
│         POST /v1/chat/completions + Authorization Header         │
│         Body: {"model": "...", "messages": [...]}                │
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
│  - Resolves skill UUID from SKILL_UUID or SKILL_NAME env vars   │
│  - Stores in ctx.filter_metadata["skill_uuid"]                  │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│  vmcp_manager Filter                                             │
│  - Creates VMCP server via skillberry-store API                 │
│  - Stores vmcp_port in ctx.filter_metadata["vmcp_port"]         │
│  - Stores vmcp_name in ctx.filter_metadata["vmcp_name"]         │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│  mcp_tools_enricher Filter (Phase 3 - NEW)                      │
│  1. Get vmcp_port and vmcp_name from ctx.filter_metadata        │
│  2. Connect to VMCP server via SSE to fetch MCP tools:          │
│     SSE: http://localhost:{vmcp_port}/sse                       │
│     Use MCP protocol to list_tools()                            │
│  3. Parse OpenAI chat completion request body                   │
│  4. Enrich request with tools schema:                           │
│     - Add "tools" array with MCP tools in OpenAI format         │
│     - Set "tool_choice": "auto" (if not already set)            │
│  5. Update request body with enriched content                   │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│  router + load_balancer                                         │
│  - Forwards enriched request to LLM backend                     │
│  - LLM receives tools schema and can call them                  │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                    LLM Backend Response                          │
│  - May include tool_calls in response                           │
│  - Tool execution handled by client/agent                       │
└─────────────────────────────────────────────────────────────────┘
```

## Usage

### Starting Praxis

```bash
# Build Praxis
cargo build --release

# Set environment variables
export SKILL_NAME="flight_reservation_management"

# Start Praxis with Phase 3 configuration
./target/release/praxis -c examples/configs/ai/phase3-mcp-tools-integration.yaml
```

### Python Client with LiteLLM Package

```python
#!/usr/bin/env python3
import os
from litellm import completion

# Set OPENAI_API_KEY with your API key

# Configure to use Praxis proxy
os.environ["OPENAI_API_BASE"] = "http://localhost:8080/v1"

response = completion(
    model="openai/rits/openai/gpt-oss-120b",
    messages=[{"role": "user", "content": "What tools do you have available?"}],
    extra_headers={"x-skillberry-env-id": "test-env-123"}
)

print(response.choices[0].message.content)

# The LLM will now see the MCP tools and can call them
# Example response might include tool_calls if the LLM decides to use them
```

**Installation**:
```bash
pip install litellm
```

**Run**:
```bash
export OPENAI_API_KEY="your-api-key"  # Set your LLM provider API key
python test_praxis_phase3.py
```

**What's Different in Phase 3**:
- ✨ **NEW**: Request is automatically enriched with MCP tools from VMCP server
- ✨ **NEW**: LLM receives `tools` array in OpenAI function calling format
- ✨ **NEW**: LLM can now call tools in its response
- Same client code as Phase 2 - enrichment is transparent!

## MCP Tools Schema Format

Based on `agentic_graph.py`, MCP tools are retrieved from the VMCP server and converted to OpenAI format:

### OpenAI Tool Format
```json
{
  "type": "function",
  "function": {
    "name": "tool_name",
    "description": "Tool description",
    "parameters": {
      "type": "object",
      "properties": {
        "param1": {
          "type": "string",
          "description": "Parameter description"
        },
        "param2": {
          "type": "integer",
          "description": "Another parameter"
        }
      },
      "required": ["param1"]
    }
  }
}
```

### Enriched Request Example

**Original Request**:
```json
{
  "model": "openai/rits/openai/gpt-oss-120b",
  "messages": [
    {"role": "user", "content": "Book a flight to NYC"}
  ]
}
```

**Enriched Request** (after mcp_tools_enricher):
```json
{
  "model": "openai/rits/openai/gpt-oss-120b",
  "messages": [
    {"role": "user", "content": "Book a flight to NYC"}
  ],
  "tools": [
    {
      "type": "function",
      "function": {
        "name": "search_flights",
        "description": "Search for available flights",
        "parameters": {
          "type": "object",
          "properties": {
            "origin": {"type": "string", "description": "Origin airport code"},
            "destination": {"type": "string", "description": "Destination airport code"},
            "date": {"type": "string", "description": "Travel date"}
          },
          "required": ["origin", "destination", "date"]
        }
      }
    },
    {
      "type": "function",
      "function": {
        "name": "book_flight",
        "description": "Book a specific flight",
        "parameters": {
          "type": "object",
          "properties": {
            "flight_id": {"type": "string", "description": "Flight identifier"},
            "passenger_name": {"type": "string", "description": "Passenger name"}
          },
          "required": ["flight_id", "passenger_name"]
        }
      }
    }
  ],
  "tool_choice": "auto"
}
```

## Implementation Plan

### 1. Create `mcp_tools_enricher` Filter

**Location**: `filter/src/builtins/http/ai/mcp_tools_enricher/`

**Files**:
- `mod.rs` - Module exports
- `config.rs` - Configuration struct
- `filter.rs` - Main filter implementation
- `tests.rs` - Unit tests

**Configuration**:
```yaml
filter: mcp_tools_enricher
timeout_ms: 5000
tool_choice: "auto"  # or "required", "none"
```

**Filter Logic**:
1. Read `vmcp_port` and `vmcp_name` from `ctx.filter_metadata`
2. Connect to VMCP server via SSE transport:
   ```
   SSE: http://localhost:{vmcp_port}/sse
   ```
3. Use MCP protocol to call `list_tools()` and get tools schema
4. Convert MCP tools to OpenAI function calling format
5. Parse incoming request body (JSON)
6. Add/merge tools into request:
   - If `tools` field exists, merge with MCP tools
   - If `tools` field doesn't exist, add it
   - Set `tool_choice` if not already set
7. Serialize updated request body
8. Update `ctx.request_body` with enriched content

### 2. VMCP Server Connection

The filter connects **directly to the VMCP server** via SSE transport, not through skillberry-store API.

**Connection Details**:
- Protocol: Server-Sent Events (SSE)
- URL: `http://localhost:{vmcp_port}/sse`
- Transport: MCP over SSE
- Method: Call MCP `list_tools()` to retrieve available tools

**MCP Tools Response Format**:
```json
{
  "tools": [
    {
      "type": "function",
      "function": {
        "name": "tool_name",
        "description": "...",
        "parameters": {...}
      }
    }
  ]
}
```

### 3. Integration with Phase 2

Update `phase2-skillberry-integration.yaml` to include the new filter:

```yaml
filter_chains:
  - name: skillberry_chain
    filters:
      - filter: context_extractor
        # ... existing config ...
      
      - filter: skill_resolver
        # ... existing config ...
      
      - filter: vmcp_manager
        # ... existing config ...
      
      # NEW: Enrich request with MCP tools
      - filter: mcp_tools_enricher
        timeout_ms: 5000
        tool_choice: "auto"
      
      - filter: router
        # ... existing config ...
      
      - filter: load_balancer
        # ... existing config ...
```

## Request Flow Example

### Step 1: Client Request
```bash
export OPENAI_API_KEY="your-api-key"  # Set your LLM provider API key

curl http://localhost:8080/v1/chat/completions \
  -H "Authorization: Bearer $OPENAI_API_KEY" \
  -H "x-skillberry-env-id: test-env-123" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "openai/rits/openai/gpt-oss-120b",
    "messages": [
      {"role": "user", "content": "Book a flight to NYC"}
    ]
  }'
```

### Step 2: After context_extractor
- `ctx.filter_metadata["env_id"]` = "test-env-123"

### Step 3: After skill_resolver
- `ctx.filter_metadata["skill_uuid"]` = "869ff6a0-acb4-4759-a065-4cb2ae43a5f3"

### Step 4: After vmcp_manager
- `ctx.filter_metadata["vmcp_port"]` = 10000
- `ctx.filter_metadata["vmcp_name"]` = "vmcp-test-env-123"

### Step 5: mcp_tools_enricher
1. Connect to VMCP server via SSE: `http://localhost:10000/sse`
2. Send MCP `list_tools()` request via SSE
3. Receive tools: `[{search_flights}, {book_flight}]`
4. Parse request body
5. Add tools to request in OpenAI format
6. Update request body

### Step 6: Forward to LLM
```json
{
  "model": "openai/rits/openai/gpt-oss-120b",
  "messages": [
    {"role": "user", "content": "Book a flight to NYC"}
  ],
  "tools": [
    {"type": "function", "function": {"name": "search_flights", ...}},
    {"type": "function", "function": {"name": "book_flight", ...}}
  ],
  "tool_choice": "auto"
}
```

### Step 7: LLM Response
```json
{
  "choices": [{
    "message": {
      "role": "assistant",
      "content": null,
      "tool_calls": [{
        "id": "call_123",
        "type": "function",
        "function": {
          "name": "search_flights",
          "arguments": "{\"origin\":\"LAX\",\"destination\":\"JFK\",\"date\":\"2024-03-15\"}"
        }
      }]
    }
  }]
}
```

## Key Design Decisions

1. **Tools Retrieval**: Connect directly to VMCP server via SSE (not through skillberry-store API)
   - Direct MCP protocol communication
   - Real-time tool discovery from VMCP server
   - Requires MCP client implementation in Rust
   - More complex than HTTP GET but provides direct access to MCP capabilities

2. **Request Enrichment**: Modify request body before forwarding to LLM
   - Transparent to client
   - LLM receives complete context
   - No client-side changes needed

3. **Tool Choice**: Configurable via filter config
   - `auto`: LLM decides when to use tools
   - `required`: LLM must use at least one tool
   - `none`: Tools provided but not required

4. **Error Handling**:
   - If tools fetch fails: Log warning, continue without tools
   - If request parsing fails: Return 400 Bad Request
   - If enrichment fails: Log error, forward original request

## Testing Strategy

### Unit Tests
- Parse OpenAI chat completion request
- Connect to VMCP server via SSE and fetch tools
- Convert MCP tools to OpenAI format
- Merge tools into request body
- Handle missing vmcp_port/vmcp_name
- Handle SSE connection errors gracefully
- Handle MCP protocol errors

### Integration Tests
- End-to-end flow with real VMCP server
- Verify tools are added to request
- Verify LLM receives enriched request
- Test with multiple tools
- Test with existing tools in request

## Future Enhancements (Phase 4+)

1. **Tool Execution**: Intercept LLM responses with tool_calls and execute them
2. **Multi-turn Conversations**: Handle tool results and continue conversation
3. **Tool Caching**: Cache tools per VMCP server to reduce API calls
4. **Prompt Enrichment**: Add MCP prompts to system messages
5. **Streaming Support**: Handle streaming responses with tool calls

## Implementation Notes

### MCP Tools Fetching Logic (Python Reference)

The Python implementation in `skillberry_store.py` (lines 325-383) shows the exact flow:

```python
from langchain_mcp_adapters.client import MultiServerMCPClient

# 1. Construct MCP server URL
mcp_server_base_url = f"http://localhost:{port}"

# 2. Build client configuration
client_config = {
    server_name: {
        "url": f"{mcp_server_base_url}/sse",
        "transport": "sse",
    }
}

# 3. Create MCP client with optional interceptors
client = MultiServerMCPClient(client_config, tool_interceptors=tool_interceptors)

# 4. Get tools from the MCP server
tools = await client.get_tools()  # Returns LangChain tools in OpenAI format
```

### Rust Implementation Status

**Current Status**: ✅ **Fully Implemented and Compiled**

**Completed**:
- ✅ Filter configuration (`config.rs`)
- ✅ Filter structure and body enrichment logic (`mod.rs`)
- ✅ Request body parsing and tools array injection
- ✅ Tool choice configuration support
- ✅ MCP SSE client using official `mcp-client` SDK
- ✅ SSE connection to `http://localhost:{vmcp_port}/sse`
- ✅ MCP protocol message exchange (initialize, list_tools)
- ✅ MCP tool response parsing
- ✅ Tool conversion from MCP to OpenAI function calling format
- ✅ Filter registration in registry
- ✅ Unit tests for enrichment logic
- ✅ Example configuration YAML
- ✅ Example flow documentation

**Implementation Details**:
- Uses official MCP Rust SDK: `mcp-client = "0.1"` and `mcp-spec = "0.1"`
- SSE transport via `SseTransport::new()`
- MCP client initialization with `McpClient::new()`
- Tool fetching via `client.list_tools()`
- Tool name prefixing with VMCP server name for disambiguation

### MCP SSE Client Implementation Guide

**Available Rust MCP Client Libraries**:

Several production-ready MCP client SDKs exist in Rust:

1. **Official SDK** - `mcp-client` / `rmcp`
   - Repository: `modelcontextprotocol/rust-sdk`
   - Full production-ready implementation
   - Supports: list resources, read files, pagination, server tools
   - **Recommended for official support**

2. **rust-mcp-sdk** (Rust MCP Stack)
   - High-performance, asynchronous toolkit
   - Procedural macros and type-safe schemas
   - Full client and server support

3. **pmcp**
   - Production-grade SDK
   - Supports: stdio, HTTP streaming, WebSocket transports
   - **Best for SSE/HTTP streaming use case**

4. **turul-mcp-client**
   - Automatic session management
   - Multiple transport options
   - Comprehensive error recovery

5. **mcp_client_rs**
   - Async client powered by Tokio
   - stdio-based transport

**Recommended Implementation**:

Use **pmcp** or **official mcp-client** for SSE transport:

```toml
[dependencies]
# Option 1: Official SDK
mcp-client = "0.1"  # Check latest version

# Option 2: pmcp for HTTP streaming
pmcp = "0.1"  # Check latest version

tokio = { version = "1", features = ["full"] }
serde_json = "1.0"
```

**Implementation Steps**:

1. Add MCP client dependency to `filter/Cargo.toml`
2. Create MCP client instance with SSE transport
3. Connect to `http://localhost:{vmcp_port}/sse`
4. Call `list_tools()` method
5. Convert MCP tool schema to OpenAI format
6. Return tools array

**Example Usage** (pseudocode):
```rust
use mcp_client::{Client, Transport};

async fn fetch_mcp_tools(port: u16) -> Result<Vec<Tool>, Error> {
    // Create SSE transport
    let transport = Transport::sse(format!("http://localhost:{}/sse", port));
    
    // Create client
    let client = Client::new(transport).await?;
    
    // List tools
    let tools = client.list_tools().await?;
    
    // Convert to OpenAI format
    tools.into_iter()
        .map(|t| convert_mcp_to_openai(t))
        .collect()
}
```

**Next Steps**:
1. Research which Rust MCP client library best fits Praxis architecture
2. Add dependency to `filter/Cargo.toml`
3. Implement `fetch_mcp_tools()` function using chosen library
4. Test with actual VMCP server

**MCP Tool Format Conversion**:
```rust
// MCP Tool (from server) → OpenAI Format (for LLM)
// {
//   "name": "tool_name",
//   "description": "...",
//   "inputSchema": { /* JSON Schema */ }
// }
// ↓
// {
//   "type": "function",
//   "function": {
//     "name": "tool_name",
//     "description": "...",
//     "parameters": { /* JSON Schema */ }
//   }
// }
```

### Integration with Existing Filters

The `mcp_tools_enricher` filter should be placed **after** `vmcp_manager` in the filter chain:

```yaml
filter_chains:
  - name: skillberry_chain
    filters:
      - filter: context_extractor      # Phase 1
      - filter: skill_resolver          # Phase 2
      - filter: vmcp_manager            # Phase 2
      - filter: mcp_tools_enricher      # Phase 3 (NEW)
      - filter: router                  # Routing
      - filter: load_balancer           # Load balancing
```

## References

- `agentic_graph.py`: Lines 242-244 (get_mcp_tools)
- `agentic_graph.py`: Lines 286-303 (bind_tools to LLM)
- `agentic_graph.py`: Lines 32-112 (convert_openai_tool_to_langchain)
- `skillberry_store.py`: Lines 325-383 (get_mcp_tools implementation)
- `mcp_interceptor.py`: Lines 160-203 (get_mcp_tools wrapper)