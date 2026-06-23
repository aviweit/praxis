// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Praxis Contributors

//! MCP tools enricher filter: fetches tools from VMCP server and injects them
//! into OpenAI-compatible chat completion request bodies.

mod config;

#[cfg(test)]
#[expect(clippy::allow_attributes, reason = "blanket test suppressions")]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "tests"
)]
mod tests;

use std::borrow::Cow;
use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use mcp_client::{ClientCapabilities, ClientInfo, McpClient, McpClientTrait, McpService, transport::{SseTransport, Transport}};
use tracing::{debug, info, warn};

use self::config::{InvalidBodyBehavior, McpToolsEnricherConfig, validate_config};
use crate::{
    FilterAction, FilterError, Rejection,
    body::{BodyAccess, BodyMode},
    factory::parse_filter_config,
    filter::{HttpFilter, HttpFilterContext},
};

// -----------------------------------------------------------------------------
// McpToolsEnricherFilter
// -----------------------------------------------------------------------------

/// Fetches MCP tools from a VMCP server and injects them into the
/// `tools` array of OpenAI-compatible chat completion request bodies.
///
/// The filter reads `vmcp_port` and `vmcp_name` from the filter metadata
/// (set by the `vmcp_manager` filter), connects to the VMCP server via SSE,
/// fetches available tools using the MCP protocol, and adds them to the
/// request body in OpenAI function calling format.
///
/// # YAML configuration
///
/// ```yaml
/// filter: mcp_tools_enricher
/// timeout_ms: 5000
/// tool_choice: auto
/// max_body_bytes: 10485760
/// on_invalid: continue
/// ```
///
/// # Example
///
/// ```rust
/// use praxis_filter::McpToolsEnricherFilter;
///
/// let yaml: serde_yaml::Value = serde_yaml::from_str(
///     r#"
/// timeout_ms: 5000
/// tool_choice: auto
/// "#,
/// )
/// .unwrap();
/// let filter = McpToolsEnricherFilter::from_config(&yaml).unwrap();
/// assert_eq!(filter.name(), "mcp_tools_enricher");
/// ```
pub struct McpToolsEnricherFilter {
    /// Maximum request body size to buffer.
    max_body_bytes: usize,

    /// Behavior when the body cannot be enriched.
    on_invalid: InvalidBodyBehavior,

    /// Timeout for MCP server connection.
    timeout: Duration,

    /// Tool choice value to set in the enriched request.
    tool_choice: String,
}

impl McpToolsEnricherFilter {
    /// Create from parsed YAML config.
    ///
    /// Validates the config at construction time.
    ///
    /// # Errors
    ///
    /// Returns [`FilterError`] if config parsing or validation fails.
    ///
    /// [`FilterError`]: crate::FilterError
    pub fn from_config(config: &serde_yaml::Value) -> Result<Box<dyn HttpFilter>, FilterError> {
        let cfg: McpToolsEnricherConfig = parse_filter_config("mcp_tools_enricher", config)?;
        validate_config(&cfg)?;

        Ok(Box::new(Self {
            max_body_bytes: cfg.max_body_bytes,
            on_invalid: cfg.on_invalid,
            timeout: Duration::from_millis(cfg.timeout_ms),
            tool_choice: cfg.tool_choice,
        }))
    }
}

#[async_trait]
impl HttpFilter for McpToolsEnricherFilter {
    fn name(&self) -> &'static str {
        "mcp_tools_enricher"
    }

    async fn on_request(&self, ctx: &mut HttpFilterContext<'_>) -> Result<FilterAction, FilterError> {
        info!("mcp_tools_enricher: on_request called");
        
        // Get VMCP server details from metadata (set by vmcp_manager)
        let vmcp_port = match ctx.filter_metadata.get("vmcp_port") {
            Some(port) => port,
            None => {
                info!("mcp_tools_enricher: vmcp_port not found in metadata, skipping tools enrichment");
                return Ok(FilterAction::Continue);
            }
        };

        let vmcp_name = ctx.filter_metadata.get("vmcp_name").map(String::as_str);
        
        info!("mcp_tools_enricher: fetching tools from VMCP server port={} name={:?}", vmcp_port, vmcp_name);

        // Fetch tools from VMCP server
        let tools = match fetch_mcp_tools(vmcp_port, vmcp_name, self.timeout).await {
            Ok(tools) => tools,
            Err(e) => {
                warn!("mcp_tools_enricher: failed to fetch MCP tools from VMCP server: {}", e);
                // Continue without tools on error
                return Ok(FilterAction::Continue);
            }
        };

        if tools.is_empty() {
            info!("mcp_tools_enricher: no tools retrieved from VMCP server");
            return Ok(FilterAction::Continue);
        }

        info!("mcp_tools_enricher: retrieved {} tools from VMCP server", tools.len());
        
        // Store tools in metadata for body enrichment
        ctx.filter_metadata.insert("mcp_tools".to_string(), serde_json::to_string(&tools).unwrap_or_default());
        ctx.filter_metadata.insert("mcp_tool_choice".to_string(), self.tool_choice.clone());

        Ok(FilterAction::Continue)
    }

    fn request_body_access(&self) -> BodyAccess {
        BodyAccess::ReadWrite
    }

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
            return Ok(FilterAction::Continue);
        };

        // Get tools from metadata (set by on_request)
        let tools_json = match ctx.filter_metadata.get("mcp_tools") {
            Some(json) => json,
            None => return Ok(FilterAction::Continue),
        };
        
        let tools: Vec<serde_json::Value> = match serde_json::from_str(tools_json) {
            Ok(t) => t,
            Err(e) => {
                warn!("mcp_tools_enricher: failed to parse tools from metadata: {}", e);
                return Ok(FilterAction::Continue);
            }
        };
        
        let tool_choice = ctx.filter_metadata.get("mcp_tool_choice")
            .map(String::as_str)
            .unwrap_or("auto");

        info!("mcp_tools_enricher: enriching request body with {} tools", tools.len());

        // Parse the request body
        let mut value: serde_json::Value = match serde_json::from_slice(raw) {
            Ok(v) => v,
            Err(e) => {
                warn!("mcp_tools_enricher: failed to parse request body as JSON: {}", e);
                return Ok(invalid_body_action(self.on_invalid, "invalid JSON body"));
            }
        };

        // Enrich the request body with tools
        enrich_request_with_tools(&mut value, tools, tool_choice)?;

        // Serialize the modified body
        let serialized = serde_json::to_vec(&value)
            .map_err(|e| -> FilterError { format!("mcp_tools_enricher: {e}").into() })?;

        let len = serialized.len();
        *body = Some(Bytes::from(serialized));

        ctx.extra_request_headers
            .push((Cow::Borrowed("content-length"), len.to_string()));

        info!("mcp_tools_enricher: request body enriched successfully");

        Ok(FilterAction::Continue)
    }
}

// -----------------------------------------------------------------------------
// Private Utilities
// -----------------------------------------------------------------------------

/// Fetch MCP tools from the VMCP server via SSE.
///
/// Connects to the VMCP server using the MCP protocol over SSE transport,
/// calls list_tools(), and converts the tools to OpenAI function calling format.
async fn fetch_mcp_tools(
    vmcp_port: &str,
    vmcp_name: Option<&str>,
    timeout: Duration,
) -> Result<Vec<serde_json::Value>, FilterError> {
    debug!(
        "Fetching tools from VMCP server at port {} (name: {:?})",
        vmcp_port, vmcp_name
    );
    
    // Build SSE endpoint URL
    let sse_url = format!("http://localhost:{}/sse", vmcp_port);
    
    // Create SSE transport with empty environment
    let env = HashMap::new();
    let transport = SseTransport::new(&sse_url, env);
    
    // Start the transport to get a handle
    let transport_handle = transport.start()
        .await
        .map_err(|e| -> FilterError {
            format!("Failed to start SSE transport: {}", e).into()
        })?;
    
    // Create MCP service from transport handle
    let service = McpService::new(transport_handle);
    
    // Create MCP client
    let mut client = McpClient::new(service);
    
    // Prepare client info
    let client_info = ClientInfo {
        name: "praxis-mcp-tools-enricher".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    
    let capabilities = ClientCapabilities::default();
    
    // Initialize the connection with timeout
    let init_result = tokio::time::timeout(timeout, client.initialize(client_info, capabilities))
        .await
        .map_err(|_| -> FilterError { "MCP initialization timeout".into() })?
        .map_err(|e| -> FilterError {
            format!("MCP initialization failed: {}", e).into()
        })?;
    
    debug!("MCP client initialized: server={}, version={}",
           init_result.server_info.name,
           init_result.server_info.version);
    
    // List available tools
    let tools_result = tokio::time::timeout(timeout, client.list_tools(None))
        .await
        .map_err(|_| -> FilterError { "MCP list_tools timeout".into() })?
        .map_err(|e| -> FilterError {
            format!("MCP list_tools failed: {}", e).into()
        })?;
    
    debug!("Retrieved {} tools from MCP server", tools_result.tools.len());
    
    // Convert MCP tools to OpenAI format
    let openai_tools: Vec<serde_json::Value> = tools_result
        .tools
        .into_iter()
        .map(|tool| convert_mcp_tool_to_openai(tool, vmcp_name))
        .collect();
    
    Ok(openai_tools)
}

/// Convert an MCP tool to OpenAI function calling format.
///
/// MCP tools have a schema that needs to be mapped to OpenAI's function format:
/// ```json
/// {
///   "type": "function",
///   "function": {
///     "name": "tool_name",
///     "description": "tool description",
///     "parameters": { ... }
///   }
/// }
/// ```
fn convert_mcp_tool_to_openai(
    tool: mcp_spec::tool::Tool,
    vmcp_name: Option<&str>,
) -> serde_json::Value {
    // Build function name with optional VMCP prefix
    let function_name = if let Some(name) = vmcp_name {
        format!("{}_{}", name, tool.name)
    } else {
        tool.name.clone()
    };
    
    serde_json::json!({
        "type": "function",
        "function": {
            "name": function_name,
            "description": tool.description,
            "parameters": tool.input_schema
        }
    })
}

/// Enrich the request body with MCP tools.
///
/// Adds or merges the tools array and sets tool_choice if not already present.
fn enrich_request_with_tools(
    value: &mut serde_json::Value,
    tools: Vec<serde_json::Value>,
    tool_choice: &str,
) -> Result<(), FilterError> {
    let obj = value
        .as_object_mut()
        .ok_or_else(|| -> FilterError { "request body is not a JSON object".into() })?;

    // Add or merge tools array
    let tools_count = tools.len();
    match obj.get_mut("tools") {
        Some(existing_tools) => {
            // Merge with existing tools
            if let Some(existing_array) = existing_tools.as_array_mut() {
                existing_array.extend(tools);
                debug!("Merged {} MCP tools with existing tools", tools_count);
            } else {
                warn!("Existing 'tools' field is not an array, replacing it");
                obj.insert("tools".to_owned(), serde_json::Value::Array(tools));
            }
        }
        None => {
            // Add new tools array
            obj.insert("tools".to_owned(), serde_json::Value::Array(tools));
            debug!("Added {} MCP tools to request", tools_count);
        }
    }

    // Set tool_choice if not already present
    if !obj.contains_key("tool_choice") {
        obj.insert(
            "tool_choice".to_owned(),
            serde_json::Value::String(tool_choice.to_owned()),
        );
        debug!("Set tool_choice to '{}'", tool_choice);
    }

    Ok(())
}

/// Map [`InvalidBodyBehavior`] to the appropriate [`FilterAction`].
fn invalid_body_action(behavior: InvalidBodyBehavior, message: &'static str) -> FilterAction {
    match behavior {
        InvalidBodyBehavior::Continue => FilterAction::Continue,
        InvalidBodyBehavior::Reject => FilterAction::Reject(
            Rejection::status(400)
                .with_header("content-type", "text/plain")
                .with_body(message),
        ),
    }
}

// Made with Bob
