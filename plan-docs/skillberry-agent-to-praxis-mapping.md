# Design Plan: Mapping Skillberry-Agent (Proxy-Agent) Logic to Praxis Filters

## Overview
Map the `/v1/chat/completions` endpoint logic from skillberry-agent (FastAPI Python) to Praxis filter pipeline (Rust).

---

## Skillberry-Agent Flow Analysis

### Current Flow (api_server.py:194-284)
```
1. Receive POST /v1/chat/completions
2. Extract Skillberry-Context headers (env_id, etc.)
3. Parse ChatRequest (model, messages, tools, temperature, etc.)
4. Execute agentic_graph workflow:
   a. Resolve skill UUID (from env vars or chat history)
   b. Create/get VMCP server for skill
   c. Get MCP tools from server
   d. Convert OpenAI tools to LangChain format
   e. Bind tools to LLM
   f. Create React workflow (LangGraph)
   g. Inject MCP prompts into messages
   h. Execute graph and stream results
   i. Build response with tool calls
5. Return OpenAI-compatible response
```

---

## Praxis Filter Pipeline Design

### Entry Point
```yaml
listeners:
  - name: skillberry-gateway
    address: "0.0.0.0:8080"
    filter_chains: [skillberry-pipeline]
```

### Filter Chain Mapping

#### **Filter 1: Header Extraction & Context Validation**
**Purpose**: Extract and validate Skillberry-Context headers  
**Praxis Equivalent**: Custom filter or use existing header manipulation

```yaml
- filter: header_extract
  # Extract nested headers like skillberry-context-env-id
  extract:
    - source: "skillberry-context-env-id"
      target: "x-praxis-env-id"
      default: "default"
  # Note: skill_uuid comes from environment variable, not headers
```

**Python Logic**:
```python
skillberry_context = unflatten_keys(headers).get(SKILLBERRY_CONTEXT.lower())
if skillberry_context is None:
    skillberry_context = {"env_id": "default"}
# Skill UUID is read from environment variable in execute_agentic_graph:
# env_skill_uuid = os.environ.get("SKILL_UUID")
# env_skill_name = os.environ.get("SKILL_NAME")
```

**Praxis Implementation**: New filter `context_extractor`
- Reads nested headers (dot notation)
- Provides defaults for env_id
- Promotes to internal headers
- **Note**: Skill UUID resolution happens in Filter 3 from environment variables, not headers

---

#### **Filter 2: Body Parsing & Tool Extraction**
**Purpose**: Parse OpenAI chat completion request, extract tools  
**Praxis Equivalent**: Extend existing body parsing or create new filter

```yaml
- filter: openai_chat_parser
  max_body_bytes: 10485760
  extract:
    - field: "model"
      header: "x-praxis-model"
    - field: "tools"
      metadata: "chat.tools"
    - field: "messages"
      metadata: "chat.messages"
```

**Python Logic**:
```python
chat_request = ChatRequest(model="...", messages=[...], tools=[...])
chat_request_tools = convert_tools_for_binding(chat_request.tools)
```

**Praxis Implementation**: New filter `openai_request_parser`
- Parses OpenAI chat completion format
- Extracts tools array
- Stores messages for later enrichment
- Similar to existing `prompt_enrich` but for full request

---

#### **Filter 3: Skill Resolution**
**Purpose**: Resolve skill UUID from multiple sources
**Praxis Equivalent**: New filter with external service callout

```yaml
- filter: skill_resolver
  resolution_priority:
    - env_var: "SKILL_UUID"      # Highest priority
    - env_var: "SKILL_NAME"      # Medium priority (requires lookup)
    - search_api: "http://skillberry-store:8000/search"  # Lowest priority (from chat history)
  fallback: "none"  # Create VMCP without skill
```

**Python Logic**:
```python
# From execute_agentic_graph (agentic_graph.py:170-224)
env_skill_uuid = os.environ.get("SKILL_UUID")
env_skill_name = os.environ.get("SKILL_NAME")

resolved_skill_uuid = resolve_skill_uuid(
    skill_uuid=env_skill_uuid,        # Priority 1: Direct UUID from env
    skill_name=env_skill_name,        # Priority 2: Name lookup from env
    chat_history=chat_messages        # Priority 3: Search from chat history
)
```

**Praxis Implementation**: New filter `skill_resolver`
- **Priority 1**: Checks `SKILL_UUID` environment variable (direct UUID)
- **Priority 2**: Checks `SKILL_NAME` environment variable (requires skillberry-store lookup)
- **Priority 3**: Extracts search term from chat history and calls skillberry-store API
- Stores result in metadata: `skill.uuid`
- **Note**: No header-based resolution in current implementation

---

#### **Filter 4: VMCP Server Management**
**Purpose**: Create or retrieve Virtual MCP server  
**Praxis Equivalent**: External service callout filter

```yaml
- filter: ext_proc  # Use existing external processor
  service: "http://vmcp-manager:8001/get_or_create"
  request_headers: true
  request_body: false
  timeout: 5s
  metadata_to_send:
    - "x-praxis-env-id"
    - "skill.uuid"
```

**Python Logic**:
```python
vmcp_data = get_or_create_vmcp_server(
    skillberry_context,
    skill_uuid=resolved_skill_uuid
)
server = VirtualMcpServer(**vmcp_data)
```

**Praxis Implementation**: Use `ext_proc` filter
- Calls VMCP manager service
- Receives server details (port, name)
- Stores in metadata: `vmcp.port`, `vmcp.name`

---

#### **Filter 5: MCP Tools Retrieval**
**Purpose**: Get tools from MCP server with interceptor  
**Praxis Equivalent**: MCP client filter or ext_proc

```yaml
- filter: mcp_tools_fetch
  server_port_from_metadata: "vmcp.port"
  server_name_from_metadata: "vmcp.name"
  method: "tools/list"
  store_in_metadata: "mcp.tools"
```

**Python Logic**:
```python
tools = get_mcp_tools(
    port=port,
    server_name=server.name,
    skillberry_context=skillberry_context
)
```

**Praxis Implementation**: New filter `mcp_client`
- Connects to MCP server (from metadata)
- Calls `tools/list`
- Stores tools in metadata
- Similar to existing MCP broker but as client

---

#### **Filter 6: Prompt Enrichment with MCP Prompts**
**Purpose**: Inject MCP prompts into messages  
**Praxis Equivalent**: Extend `prompt_enrich` filter

```yaml
- filter: prompt_enrich
  max_body_bytes: 10485760
  mcp_prompts:
    source_metadata: "mcp.prompts"
    position: "postfix"  # or "prefix"
    enabled_from_env: "USE_AGENT_PROMPTS"
  prepend:
    - role: system
      content: "You are a helpful assistant."
  append:
    - role: user
      content_from_metadata: "mcp.prompts"
```

**Python Logic**:
```python
chat_messages = build_chat_messages(
    chat_messages,
    mcp_prompts,
    position=env_mcp_prompts_position
)
```

**Praxis Implementation**: Enhance existing `prompt_enrich`
- Support dynamic content from metadata
- Support position control (prefix/postfix)
- Support conditional enablement

---

#### **Filter 7: Tool Binding & LLM Routing**
**Purpose**: Route to appropriate LLM with tools bound  
**Praxis Equivalent**: Router with tool-aware routing

```yaml
- filter: router
  routes:
    - condition:
        metadata_exists: "mcp.tools"
      cluster: "llm-with-tools"
      inject_tools_from_metadata: "mcp.tools"
    - path_prefix: "/v1/chat/completions"
      cluster: "llm-default"

- filter: load_balancer
  clusters:
    - name: "llm-with-tools"
      endpoints:
        - "openai-api:443"
      tls: true
    - name: "llm-default"
      endpoints:
        - "openai-api:443"
      tls: true
```

**Python Logic**:
```python
llm_with_tools = current_llm().bind_tools(tools)
workflow = create_react_tools_workflow(llm_with_tools)
```

**Praxis Implementation**: 
- Router selects cluster based on tool availability
- New feature: inject tools into request body before forwarding

---

#### **Filter 8: Response Processing**
**Purpose**: Build OpenAI-compatible response with tool calls  
**Praxis Equivalent**: Response transformation filter

```yaml
- filter: response_transformer
  format: "openai_chat_completion"
  add_fields:
    id: "chatcmpl-{uuid}"
    object: "chat.completion"
    created: "{timestamp}"
    model: "skillberry"
    usage:
      prompt_tokens: 1
      completion_tokens: 1
      total_tokens: 2
```

**Python Logic**:
```python
response = {
    "id": f"chatcmpl-{uuid.uuid4().hex[:24]}",
    "object": "chat.completion",
    "created": int(time.time()),
    "model": "skillberry",
    "choices": [{"index": 0, "message": message_dict, "finish_reason": finish_reason}],
    "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
}
```

**Praxis Implementation**: New filter `openai_response_builder`
- Wraps LLM response in OpenAI format
- Adds metadata fields
- Handles tool calls in response

---

## Complete Praxis Configuration

```yaml
listeners:
  - name: skillberry-gateway
    address: "0.0.0.0:8080"
    filter_chains: [skillberry-pipeline]

filter_chains:
  - name: skillberry-pipeline
    filters:
      # 1. Extract context headers
      - filter: context_extractor
        extract:
          - source: "skillberry-context-env-id"
            target: "x-praxis-env-id"
            default: "default"
        # Note: skill_uuid comes from environment variables, not headers

      # 2. Parse OpenAI request
      - filter: openai_request_parser
        max_body_bytes: 10485760
        extract_to_metadata:
          - field: "model"
          - field: "tools"
          - field: "messages"

      # 3. Resolve skill UUID
      - filter: skill_resolver
        resolution_priority:
          - env_var: "SKILL_UUID"
          - header: "x-praxis-skill-uuid"
          - search_api: "http://skillberry-store:8000/search"

      # 4. Get/Create VMCP server
      - filter: ext_proc
        service: "http://vmcp-manager:8001/get_or_create"
        timeout: 5s

      # 5. Fetch MCP tools
      - filter: mcp_client
        server_from_metadata: "vmcp"
        method: "tools/list"

      # 6. Enrich prompts
      - filter: prompt_enrich
        mcp_prompts_from_metadata: "mcp.prompts"
        position: "postfix"

      # 7. Route to LLM
      - filter: router
        routes:
          - path_prefix: "/v1/chat/completions"
            cluster: "openai-llm"

      # 8. Load balance
      - filter: load_balancer
        clusters:
          - name: "openai-llm"
            endpoints:
              - "api.openai.com:443"
            tls: true

      # 9. Transform response
      - filter: openai_response_builder
        format: "chat_completion"
```

---

## New Filters Required

1. **`context_extractor`**: Extract nested headers with dot notation
2. **`openai_request_parser`**: Parse OpenAI chat completion format
3. **`skill_resolver`**: Multi-strategy skill UUID resolution
4. **`mcp_client`**: MCP client (not broker) for tools/list
5. **`openai_response_builder`**: Build OpenAI-compatible responses

## Filters to Enhance

1. **`prompt_enrich`**: Add dynamic content from metadata, position control
2. **`ext_proc`**: Already exists, use for VMCP manager calls
3. **`router`**: Add tool injection capability

---

## Implementation Priority

### Phase 1: Basic Flow Without MCP
- context_extractor
- openai_request_parser
- router → LLM
- openai_response_builder

### Phase 2: Add Skill Resolution
- skill_resolver
- ext_proc for VMCP manager

### Phase 3: Add MCP Integration
- mcp_client
- Enhanced prompt_enrich

### Phase 4: Advanced Features
- Tool binding in router
- Streaming support
- Trajectory tracking

---

## Architecture Comparison

### Skillberry-Agent (Python/FastAPI)
```
FastAPI → Header Extract → Parse Request → Resolve Skill → 
VMCP Manager → MCP Tools → LangGraph → LLM → Response Builder
```

### Praxis (Rust/Pingora)
```
Listener → context_extractor → openai_request_parser → skill_resolver → 
ext_proc (VMCP) → mcp_client → prompt_enrich → router → 
load_balancer → openai_response_builder
```

---

## Key Differences

1. **State Management**: 
   - Python: In-memory state in FastAPI process
   - Praxis: Metadata pipeline, external state services

2. **Tool Execution**:
   - Python: LangGraph executes tools directly
   - Praxis: Tools forwarded to LLM, execution handled by LLM or downstream

3. **Async Processing**:
   - Python: asyncio with LangGraph streaming
   - Praxis: Pingora async runtime with filter pipeline

4. **Configuration**:
   - Python: Environment variables + code
   - Praxis: YAML configuration with filter composition

---

## Migration Strategy

1. **Start Simple**: Implement basic OpenAI proxy without MCP
2. **Add Context**: Implement header extraction and context management
3. **Integrate MCP**: Add MCP client and tool retrieval
4. **Enhance Prompts**: Dynamic prompt injection from MCP
5. **Optimize**: Performance tuning, caching, connection pooling

---

## Benefits of Praxis Implementation

1. **Performance**: Rust + Pingora = lower latency, higher throughput
2. **Scalability**: Better resource utilization, horizontal scaling
3. **Reliability**: Type safety, memory safety, crash resistance
4. **Observability**: Built-in metrics, tracing, logging
5. **Configuration**: Declarative YAML vs imperative Python code
6. **Deployment**: Single binary vs Python dependencies

---

## Next Steps

1. Review existing Praxis filters for reusability
2. Design detailed specs for new filters
3. Implement Phase 1 (basic flow)
4. Test with OpenAI API
5. Iterate through remaining phases