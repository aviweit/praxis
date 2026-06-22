# Phase 1: Basic OpenAI Proxy with Context Extraction

## Overview
Implement a production-ready OpenAI-compatible proxy in Praxis that handles `/v1/chat/completions` requests with Skillberry context header extraction. This phase establishes the foundation for later MCP integration.

**Duration**: 3-5 days  
**Goal**: Production-ready OpenAI proxy with context awareness

---

## Objectives

### Primary Goals
1. ✅ Accept OpenAI chat completion requests at `/v1/chat/completions`
2. ✅ Extract and validate Skillberry-Context headers (env_id)
3. ✅ Parse OpenAI request format (model, messages, tools, parameters)
4. ✅ Forward requests to OpenAI API (or compatible LLM)
5. ✅ Build OpenAI-compatible responses
6. ✅ Handle errors gracefully

### Success Criteria
- [ ] All OpenAI chat completion parameters supported
- [ ] Skillberry-Context headers extracted correctly
- [ ] Response format matches OpenAI API exactly
- [ ] Error responses follow OpenAI error format
- [ ] Performance: <50ms overhead vs direct OpenAI call
- [ ] Integration tests pass for common scenarios

---

## Architecture

### Request Flow
```
Client Request
    ↓
[Listener: 0.0.0.0:8080]
    ↓
[Filter 1: context_extractor]
    ├─ Extract: skillberry-context-env-id → x-praxis-env-id
    └─ Default: "default" if missing
    ↓
[Filter 2: openai_request_parser]
    ├─ Parse: model, messages, tools, temperature, etc.
    ├─ Validate: Required fields present
    └─ Store: Metadata for logging/metrics
    ↓
[Filter 3: router]
    └─ Route: /v1/chat/completions → openai-llm cluster
    ↓
[Filter 4: load_balancer]
    └─ Forward: To OpenAI API endpoint
    ↓
OpenAI API
    ↓
[Filter 5: openai_response_builder]
    ├─ Validate: Response format
    ├─ Add: Praxis metadata (optional)
    └─ Transform: If needed
    ↓
Client Response
```

---

## Filter Specifications

### Filter 1: `context_extractor`

#### Purpose
Extract nested Skillberry-Context headers and provide defaults.

#### Configuration
```yaml
- filter: context_extractor
  extract:
    - source: "skillberry-context-env-id"
      target: "x-praxis-env-id"
      default: "default"
      required: false
    - source: "skillberry-context-user-id"
      target: "x-praxis-user-id"
      optional: true
  validation:
    env_id_pattern: "^[a-zA-Z0-9_-]+$"
    max_length: 64
```

#### Implementation Details

**Input**: HTTP headers with nested structure
```
skillberry-context-env-id: prod-123
skillberry-context-user-id: user-456
```

**Output**: Internal headers
```
x-praxis-env-id: prod-123
x-praxis-user-id: user-456
```

**Metadata**: Store in filter context
```rust
ctx.set_metadata("context.env_id", "prod-123");
ctx.set_metadata("context.user_id", "user-456");
```

#### Error Handling
- Missing env_id: Use default "default"
- Invalid format: Log warning, use default
- Too long: Truncate and log warning

#### Rust Implementation Sketch
```rust
pub struct ContextExtractorFilter {
    extractions: Vec<HeaderExtraction>,
    validation: ValidationRules,
}

struct HeaderExtraction {
    source: String,
    target: String,
    default: Option<String>,
    required: bool,
}

impl HttpFilter for ContextExtractorFilter {
    async fn on_request(&self, ctx: &mut HttpFilterContext<'_>) -> Result<FilterAction, FilterError> {
        for extraction in &self.extractions {
            let value = ctx.request.headers
                .get(&extraction.source)
                .and_then(|v| v.to_str().ok())
                .or(extraction.default.as_deref());
            
            if let Some(val) = value {
                // Validate
                if self.validate(val) {
                    // Promote to internal header
                    ctx.extra_request_headers.push((
                        Cow::Owned(extraction.target.clone()),
                        val.to_owned()
                    ));
                    // Store in metadata
                    ctx.set_metadata(&format!("context.{}", extraction.source), val);
                }
            } else if extraction.required {
                return Ok(FilterAction::Reject(Rejection::status(400)
                    .with_body("Missing required context header")));
            }
        }
        Ok(FilterAction::Continue)
    }
}
```

---

### Filter 2: `openai_request_parser`

#### Purpose
Parse and validate OpenAI chat completion request format.

#### Configuration
```yaml
- filter: openai_request_parser
  max_body_bytes: 10485760  # 10MB
  validation:
    required_fields: ["model", "messages"]
    max_messages: 100
    max_message_length: 32768
  extract_to_metadata:
    - field: "model"
      metadata_key: "request.model"
    - field: "temperature"
      metadata_key: "request.temperature"
    - field: "max_tokens"
      metadata_key: "request.max_tokens"
  extract_to_headers:
    - field: "model"
      header: "x-praxis-model"
```

#### OpenAI Request Format
```json
{
  "model": "gpt-4",
  "messages": [
    {"role": "system", "content": "You are a helpful assistant."},
    {"role": "user", "content": "Hello!"}
  ],
  "temperature": 0.7,
  "max_tokens": 1000,
  "top_p": 1.0,
  "frequency_penalty": 0.0,
  "presence_penalty": 0.0,
  "tools": [...],
  "tool_choice": "auto",
  "stream": false
}
```

#### Implementation Details

**Validation Rules**:
1. `model`: Required, non-empty string
2. `messages`: Required, non-empty array
3. Each message: Must have `role` and `content`
4. `temperature`: Optional, 0.0-2.0
5. `max_tokens`: Optional, positive integer
6. `tools`: Optional, array of tool definitions
7. `stream`: Optional, boolean (Phase 1: reject if true)

**Metadata Storage**:
```rust
ctx.set_metadata("request.model", "gpt-4");
ctx.set_metadata("request.message_count", "2");
ctx.set_metadata("request.has_tools", "false");
ctx.set_metadata("request.stream", "false");
```

#### Error Responses
```json
{
  "error": {
    "message": "Invalid request: missing required field 'model'",
    "type": "invalid_request_error",
    "code": "missing_field"
  }
}
```

#### Rust Implementation Sketch
```rust
pub struct OpenAIRequestParserFilter {
    max_body_bytes: usize,
    validation: ValidationRules,
    extractions: Vec<FieldExtraction>,
}

#[derive(Deserialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(default)]
    temperature: Option<f32>,
    #[serde(default)]
    max_tokens: Option<u32>,
    #[serde(default)]
    tools: Option<Vec<Tool>>,
    #[serde(default)]
    stream: bool,
}

impl HttpFilter for OpenAIRequestParserFilter {
    fn request_body_mode(&self) -> BodyMode {
        BodyMode::StreamBuffer {
            max_bytes: Some(self.max_body_bytes),
        }
    }

    async fn on_request_body(
        &self,
        ctx: &mut HttpFilterContext<'_>,
        body: &mut Option<Bytes>,
        end_of_stream: bool,
    ) -> Result<FilterAction, FilterError> {
        if !end_of_stream {
            return Ok(FilterAction::Continue);
        }

        let Some(raw) = body.as_ref() else {
            return Ok(FilterAction::Reject(
                Rejection::status(400).with_body("Empty request body")
            ));
        };

        // Parse JSON
        let request: ChatCompletionRequest = match serde_json::from_slice(raw) {
            Ok(req) => req,
            Err(e) => return Ok(openai_error_response(400, "invalid_request_error", &e.to_string())),
        };

        // Validate
        if let Err(msg) = self.validate(&request) {
            return Ok(openai_error_response(400, "invalid_request_error", msg));
        }

        // Extract to metadata
        ctx.set_metadata("request.model", &request.model);
        ctx.set_metadata("request.message_count", request.messages.len().to_string());
        ctx.set_metadata("request.has_tools", request.tools.is_some().to_string());
        
        // Phase 1: Reject streaming
        if request.stream {
            return Ok(openai_error_response(
                400,
                "invalid_request_error",
                "Streaming not supported in Phase 1"
            ));
        }

        Ok(FilterAction::Continue)
    }
}
```

---

### Filter 3: `router`

#### Purpose
Route requests to appropriate backend cluster.

#### Configuration
```yaml
- filter: router
  routes:
    - path_prefix: "/v1/chat/completions"
      cluster: "openai-llm"
    - path_prefix: "/v1/completions"
      cluster: "openai-llm"
    - path_prefix: "/health"
      cluster: "local-health"
```

#### Implementation
Use existing Praxis router filter - no custom implementation needed.

---

### Filter 4: `load_balancer`

#### Purpose
Forward requests to OpenAI API with load balancing.

#### Configuration
```yaml
- filter: load_balancer
  clusters:
    - name: "openai-llm"
      endpoints:
        - "api.openai.com:443"
      tls:
        enabled: true
        sni: "api.openai.com"
      health_check:
        enabled: false  # Phase 1: No health checks
      connection_pool:
        max_idle_per_host: 10
        idle_timeout: 90s
```

#### Implementation
Use existing Praxis load_balancer filter - no custom implementation needed.

---

### Filter 5: `openai_response_builder`

#### Purpose
Validate and optionally enhance OpenAI responses.

#### Configuration
```yaml
- filter: openai_response_builder
  validation:
    required_fields: ["id", "object", "created", "model", "choices"]
  add_headers:
    - name: "x-praxis-env-id"
      value_from_metadata: "context.env_id"
    - name: "x-praxis-request-id"
      value_from_metadata: "request.id"
  logging:
    log_response: true
    log_level: "debug"
```

#### OpenAI Response Format
```json
{
  "id": "chatcmpl-123",
  "object": "chat.completion",
  "created": 1677652288,
  "model": "gpt-4",
  "choices": [{
    "index": 0,
    "message": {
      "role": "assistant",
      "content": "Hello! How can I help you today?"
    },
    "finish_reason": "stop"
  }],
  "usage": {
    "prompt_tokens": 10,
    "completion_tokens": 9,
    "total_tokens": 19
  }
}
```

#### Implementation Details

**Validation**:
- Verify response is valid JSON
- Check required fields present
- Validate structure matches OpenAI format

**Enhancement** (Optional):
- Add custom headers (x-praxis-*)
- Log response for debugging
- Add metrics

**Error Handling**:
- If OpenAI returns error, pass through
- If response invalid, return 502 Bad Gateway

#### Rust Implementation Sketch
```rust
pub struct OpenAIResponseBuilderFilter {
    validation: ResponseValidation,
    add_headers: Vec<HeaderAddition>,
    logging: LoggingConfig,
}

impl HttpFilter for OpenAIResponseBuilderFilter {
    async fn on_response(
        &self,
        ctx: &mut HttpFilterContext<'_>,
    ) -> Result<FilterAction, FilterError> {
        // Log response metadata
        if self.logging.log_response {
            debug!(
                env_id = ctx.get_metadata("context.env_id"),
                model = ctx.get_metadata("request.model"),
                status = ctx.response.status,
                "OpenAI response received"
            );
        }

        // Add custom headers
        for header in &self.add_headers {
            if let Some(value) = ctx.get_metadata(&header.value_from_metadata) {
                ctx.extra_response_headers.push((
                    Cow::Owned(header.name.clone()),
                    value.to_owned()
                ));
            }
        }

        Ok(FilterAction::Continue)
    }

    async fn on_response_body(
        &self,
        ctx: &mut HttpFilterContext<'_>,
        body: &mut Option<Bytes>,
        end_of_stream: bool,
    ) -> Result<FilterAction, FilterError> {
        if !end_of_stream {
            return Ok(FilterAction::Continue);
        }

        let Some(raw) = body.as_ref() else {
            return Ok(FilterAction::Continue);
        };

        // Validate response format
        if let Err(e) = self.validate_response(raw) {
            warn!("Invalid OpenAI response: {}", e);
            // Could transform to valid error response here
        }

        Ok(FilterAction::Continue)
    }
}
```

---

## Complete Configuration

### Full Praxis YAML
```yaml
# Phase 1: Basic OpenAI Proxy with Context Extraction
# File: configs/phase1-openai-proxy.yaml

listeners:
  - name: skillberry-gateway
    address: "0.0.0.0:8080"
    filter_chains: [openai-proxy]

filter_chains:
  - name: openai-proxy
    filters:
      # 1. Extract Skillberry context headers
      - filter: context_extractor
        extract:
          - source: "skillberry-context-env-id"
            target: "x-praxis-env-id"
            default: "default"
            required: false
          - source: "skillberry-context-user-id"
            target: "x-praxis-user-id"
            optional: true
        validation:
          env_id_pattern: "^[a-zA-Z0-9_-]+$"
          max_length: 64

      # 2. Parse OpenAI request
      - filter: openai_request_parser
        max_body_bytes: 10485760  # 10MB
        validation:
          required_fields: ["model", "messages"]
          max_messages: 100
          max_message_length: 32768
        extract_to_metadata:
          - field: "model"
            metadata_key: "request.model"
          - field: "temperature"
            metadata_key: "request.temperature"
        extract_to_headers:
          - field: "model"
            header: "x-praxis-model"

      # 3. Route to OpenAI
      - filter: router
        routes:
          - path_prefix: "/v1/chat/completions"
            cluster: "openai-llm"
          - path_prefix: "/v1/completions"
            cluster: "openai-llm"
          - path_prefix: "/health"
            cluster: "local-health"

      # 4. Load balance to OpenAI API
      - filter: load_balancer
        clusters:
          - name: "openai-llm"
            endpoints:
              - "api.openai.com:443"
            tls:
              enabled: true
              sni: "api.openai.com"
            connection_pool:
              max_idle_per_host: 10
              idle_timeout: 90s
          - name: "local-health"
            endpoints:
              - "127.0.0.1:8081"

      # 5. Process response
      - filter: openai_response_builder
        validation:
          required_fields: ["id", "object", "created", "model", "choices"]
        add_headers:
          - name: "x-praxis-env-id"
            value_from_metadata: "context.env_id"
        logging:
          log_response: true
          log_level: "debug"
```

---

## Testing Strategy

### Unit Tests

#### Test 1: Context Extraction
```rust
#[tokio::test]
async fn test_context_extractor_with_valid_headers() {
    let filter = ContextExtractorFilter::from_config(&yaml).unwrap();
    let mut req = make_request_with_headers(&[
        ("skillberry-context-env-id", "test-env"),
    ]);
    let mut ctx = make_filter_context(&req);
    
    let action = filter.on_request(&mut ctx).await.unwrap();
    
    assert!(matches!(action, FilterAction::Continue));
    assert_eq!(ctx.get_metadata("context.env_id"), Some("test-env"));
}

#[tokio::test]
async fn test_context_extractor_missing_env_id_uses_default() {
    let filter = ContextExtractorFilter::from_config(&yaml).unwrap();
    let mut req = make_request_with_headers(&[]);
    let mut ctx = make_filter_context(&req);
    
    let action = filter.on_request(&mut ctx).await.unwrap();
    
    assert_eq!(ctx.get_metadata("context.env_id"), Some("default"));
}
```

#### Test 2: Request Parsing
```rust
#[tokio::test]
async fn test_openai_parser_valid_request() {
    let filter = OpenAIRequestParserFilter::from_config(&yaml).unwrap();
    let body = r#"{
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "Hello"}]
    }"#;
    let mut ctx = make_context_with_body(body);
    
    let action = filter.on_request_body(&mut ctx, &mut Some(body.into()), true)
        .await.unwrap();
    
    assert!(matches!(action, FilterAction::Continue));
    assert_eq!(ctx.get_metadata("request.model"), Some("gpt-4"));
}

#[tokio::test]
async fn test_openai_parser_missing_model() {
    let filter = OpenAIRequestParserFilter::from_config(&yaml).unwrap();
    let body = r#"{"messages": [{"role": "user", "content": "Hello"}]}"#;
    let mut ctx = make_context_with_body(body);
    
    let action = filter.on_request_body(&mut ctx, &mut Some(body.into()), true)
        .await.unwrap();
    
    assert!(matches!(action, FilterAction::Reject(_)));
}
```

### Integration Tests

#### Test 1: End-to-End Request
```rust
#[test]
fn test_phase1_complete_flow() {
    let backend = start_mock_openai_backend();
    let proxy = start_praxis_with_config("configs/phase1-openai-proxy.yaml");
    
    let response = http_client()
        .post(format!("http://localhost:{}/v1/chat/completions", proxy.port()))
        .header("skillberry-context-env-id", "test-env")
        .header("content-type", "application/json")
        .body(r#"{
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}]
        }"#)
        .send()
        .unwrap();
    
    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().unwrap();
    assert_eq!(body["object"], "chat.completion");
    assert!(body["choices"].is_array());
}
```

#### Test 2: Error Handling
```rust
#[test]
fn test_phase1_invalid_request() {
    let proxy = start_praxis_with_config("configs/phase1-openai-proxy.yaml");
    
    let response = http_client()
        .post(format!("http://localhost:{}/v1/chat/completions", proxy.port()))
        .header("content-type", "application/json")
        .body(r#"{"invalid": "request"}"#)
        .send()
        .unwrap();
    
    assert_eq!(response.status(), 400);
    let body: serde_json::Value = response.json().unwrap();
    assert!(body["error"].is_object());
    assert_eq!(body["error"]["type"], "invalid_request_error");
}
```

### Performance Tests

```rust
#[test]
fn test_phase1_latency_overhead() {
    let backend = start_mock_openai_backend();
    let proxy = start_praxis_with_config("configs/phase1-openai-proxy.yaml");
    
    // Measure direct backend latency
    let direct_latency = measure_latency(|| {
        call_backend_directly(&backend);
    });
    
    // Measure through proxy
    let proxy_latency = measure_latency(|| {
        call_through_proxy(&proxy);
    });
    
    let overhead = proxy_latency - direct_latency;
    assert!(overhead < Duration::from_millis(50), 
        "Overhead {} exceeds 50ms threshold", overhead);
}
```

---

## Implementation Checklist

### Week 1: Core Filters
- [ ] Day 1-2: Implement `context_extractor` filter
  - [ ] Header extraction logic
  - [ ] Default value handling
  - [ ] Validation rules
  - [ ] Unit tests
- [ ] Day 3-4: Implement `openai_request_parser` filter
  - [ ] JSON parsing
  - [ ] Validation logic
  - [ ] Metadata extraction
  - [ ] Unit tests
- [ ] Day 5: Implement `openai_response_builder` filter
  - [ ] Response validation
  - [ ] Header addition
  - [ ] Logging
  - [ ] Unit tests

### Week 2: Integration & Testing
- [ ] Day 1: Integration testing
  - [ ] End-to-end tests
  - [ ] Error scenario tests
  - [ ] Edge case tests
- [ ] Day 2: Performance testing
  - [ ] Latency benchmarks
  - [ ] Throughput tests
  - [ ] Memory profiling
- [ ] Day 3: Documentation
  - [ ] Filter documentation
  - [ ] Configuration examples
  - [ ] Troubleshooting guide
- [ ] Day 4-5: Bug fixes and polish
  - [ ] Address test failures
  - [ ] Performance optimization
  - [ ] Code review feedback

---

## Success Metrics

### Functional Metrics
- ✅ All OpenAI request parameters supported
- ✅ 100% test coverage for filters
- ✅ Error handling for all edge cases
- ✅ Logging and observability in place

### Performance Metrics
- ✅ Latency overhead: <50ms (p99)
- ✅ Throughput: >1000 req/s (single instance)
- ✅ Memory usage: <100MB baseline
- ✅ CPU usage: <10% idle, <80% under load

### Quality Metrics
- ✅ Zero crashes in 24h stress test
- ✅ No memory leaks
- ✅ All clippy warnings resolved
- ✅ Code review approved

---

## Risks & Mitigation

### Risk 1: Performance Overhead
**Impact**: High  
**Probability**: Medium  
**Mitigation**:
- Benchmark early and often
- Profile hot paths
- Optimize body parsing (zero-copy where possible)
- Use connection pooling

### Risk 2: OpenAI API Changes
**Impact**: Medium  
**Probability**: Low  
**Mitigation**:
- Version API calls
- Monitor OpenAI changelog
- Implement graceful degradation
- Add compatibility layer

### Risk 3: Complex Error Scenarios
**Impact**: Medium  
**Probability**: High  
**Mitigation**:
- Comprehensive error testing
- Clear error messages
- Fallback behaviors
- Detailed logging

---

## Next Steps After Phase 1

Once Phase 1 is complete and stable:

1. **Deploy to Staging**: Test with real traffic
2. **Monitor Metrics**: Validate performance goals
3. **Gather Feedback**: From users and developers
4. **Plan Phase 2**: Skill resolution implementation
5. **Document Lessons**: What worked, what didn't

---

## Questions & Decisions Needed

### Open Questions
1. Should we support streaming in Phase 1 or defer to Phase 5?
   - **Recommendation**: Defer to Phase 5 (simpler, faster delivery)

2. Which LLM backends to support besides OpenAI?
   - **Recommendation**: OpenAI only in Phase 1, add others in Phase 2+

3. How to handle authentication/API keys?
   - **Recommendation**: Pass through from client headers or env vars

4. Should we cache responses?
   - **Recommendation**: No caching in Phase 1, add in Phase 5

### Decisions Made
- ✅ Use existing Praxis router and load_balancer filters
- ✅ Implement 3 new custom filters
- ✅ Target <50ms latency overhead
- ✅ Defer streaming support to Phase 5
- ✅ Focus on OpenAI API compatibility only

---

## Resources

### Documentation
- [OpenAI API Reference](https://platform.openai.com/docs/api-reference/chat)
- [Praxis Filter Development Guide](../docs/filter-development.md)
- [Pingora Documentation](https://github.com/cloudflare/pingora)

### Code References
- Existing Praxis filters: `filter/src/builtins/`
- Router implementation: `filter/src/builtins/http/router/`
- Load balancer: `filter/src/builtins/http/load_balancer/`

### Tools
- Rust testing: `cargo test`
- Benchmarking: `cargo bench`
- Profiling: `cargo flamegraph`
- Load testing: `wrk`, `vegeta`, or `k6`