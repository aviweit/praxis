// SPDX-License-Identifier: MIT
// Copyright (c) 2024 Praxis Contributors

//! VMCP manager filter implementation.

use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use reqwest::Client;
use serde::Deserialize;

use super::config::VmcpManagerConfig;
use crate::{
    FilterAction, FilterError,
    body::{BodyAccess, BodyMode},
    factory::parse_filter_config,
    filter::{HttpFilter, HttpFilterContext},
};

// -----------------------------------------------------------------------------
// Response Types
// -----------------------------------------------------------------------------

/// Response from skillberry-store POST /vmcp_servers/ endpoint.
#[derive(Debug, Deserialize)]
struct VmcpResponse {
    uuid: String,
    name: String,
    port: u16,
    #[allow(dead_code)]
    skill_uuid: Option<String>,
    #[allow(dead_code)]
    runtime_tools: Option<serde_json::Value>,
}

// -----------------------------------------------------------------------------
// VmcpManagerFilter
// -----------------------------------------------------------------------------

/// Creates and manages Virtual MCP (VMCP) servers.
///
/// This filter creates VMCP servers via the skillberry-store API,
/// passing the environment context and optional skill UUID. The VMCP
/// server provides MCP tool access for the request.
///
/// # Requirements
///
/// - `env_id` must be present in `ctx.filter_metadata` (from context_extractor)
/// - `skill_uuid` is optional in metadata (from skill_resolver)
///
/// # YAML Configuration
///
/// ```yaml
/// filter: vmcp_manager
/// store_base_url: "http://localhost:8000"
/// vmcp_name_template: "vmcp-{env_id}"
/// always_create: true
/// timeout_ms: 10000
/// cleanup_on_error: true
/// ```
///
/// # Example
///
/// ```
/// use praxis_filter::VmcpManagerFilter;
///
/// let yaml: serde_yaml::Value = serde_yaml::from_str(
///     r#"
/// store_base_url: "http://localhost:8000"
/// vmcp_name_template: "vmcp-{env_id}"
/// always_create: true
/// timeout_ms: 10000
/// "#,
/// )
/// .unwrap();
/// let filter = VmcpManagerFilter::from_config(&yaml).unwrap();
/// assert_eq!(filter.name(), "vmcp_manager");
/// ```
pub struct VmcpManagerFilter {
    /// HTTP client for making requests to skillberry-store.
    http_client: Client,
    
    /// Base URL of skillberry-store.
    store_base_url: String,
    
    /// Template for VMCP server names.
    vmcp_name_template: String,
    
    /// Always create new VMCP servers.
    always_create: bool,
    
    /// HTTP request timeout.
    timeout: Duration,
    
    /// Cleanup VMCP on error.
    #[allow(dead_code)]
    cleanup_on_error: bool,
}

impl VmcpManagerFilter {
    /// Create from YAML config.
    ///
    /// # Errors
    ///
    /// Returns [`FilterError`] if:
    /// - `store_base_url` is empty
    /// - HTTP client creation fails
    ///
    /// [`FilterError`]: crate::FilterError
    pub fn from_config(config: &serde_yaml::Value) -> Result<Box<dyn HttpFilter>, FilterError> {
        let cfg: VmcpManagerConfig = parse_filter_config("vmcp_manager", config)?;

        if cfg.store_base_url.is_empty() {
            return Err("vmcp_manager: 'store_base_url' must not be empty".into());
        }

        let http_client = Client::builder()
            .timeout(Duration::from_millis(cfg.timeout_ms))
            .build()
            .map_err(|e| -> FilterError {
                format!("vmcp_manager: failed to create HTTP client: {e}").into()
            })?;

        Ok(Box::new(Self {
            http_client,
            store_base_url: cfg.store_base_url,
            vmcp_name_template: cfg.vmcp_name_template,
            always_create: cfg.always_create,
            timeout: Duration::from_millis(cfg.timeout_ms),
            cleanup_on_error: cfg.cleanup_on_error,
        }))
    }

    /// Generate VMCP server name from template.
    fn generate_vmcp_name(&self, env_id: &str) -> String {
        self.vmcp_name_template.replace("{env_id}", env_id)
    }

    /// Create a VMCP server via HTTP API.
    async fn create_vmcp_server(
        &self,
        name: &str,
        skill_uuid: Option<&str>,
        env_id: &str,
    ) -> Result<VmcpResponse, FilterError> {
        let url = format!("{}/vmcp_servers/", self.store_base_url);
        
        tracing::debug!(
            vmcp_name = %name,
            skill_uuid = ?skill_uuid,
            env_id = %env_id,
            url = %url,
            "creating VMCP server"
        );

        // Build query parameters
        let mut query_params = vec![
            ("name", name.to_string()),
            ("description", format!("VMCP server for environment {}", env_id)),
        ];
        
        if let Some(uuid) = skill_uuid {
            query_params.push(("skill_uuid", uuid.to_string()));
        }

        let mut request = self.http_client
            .post(&url)
            .header("skillberry-context-env-id", env_id)
            .query(&query_params);

        // Add timeout
        request = request.timeout(self.timeout);

        let response = request.send().await
            .map_err(|e| -> FilterError {
                if e.is_timeout() {
                    tracing::error!(vmcp_name = %name, "VMCP creation timed out");
                    FilterError::from("VMCP creation timed out")
                } else if e.is_connect() {
                    tracing::error!(
                        vmcp_name = %name,
                        error = %e,
                        "failed to connect to skillberry-store"
                    );
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "skillberry-store is unreachable",
                    ))
                } else {
                    tracing::error!(
                        vmcp_name = %name,
                        error = %e,
                        "VMCP creation request failed"
                    );
                    FilterError::from(format!("VMCP creation failed: {e}"))
                }
            })?;

        let status = response.status();
        
        if status.is_success() {
            response.json::<VmcpResponse>().await
                .map_err(|e| -> FilterError {
                    tracing::error!(
                        vmcp_name = %name,
                        error = %e,
                        "failed to parse VMCP response"
                    );
                    FilterError::from(format!("invalid VMCP response: {e}"))
                })
        } else if status.as_u16() == 409 {
            // Conflict - VMCP already exists
            if self.always_create {
                tracing::error!(
                    vmcp_name = %name,
                    "VMCP already exists but always_create is true"
                );
                Err(FilterError::from(format!("VMCP '{}' already exists", name)))
            } else {
                // Phase 3: Reuse existing VMCP
                tracing::info!(
                    vmcp_name = %name,
                    "VMCP already exists, reusing"
                );
                // TODO: Fetch existing VMCP details
                Err(FilterError::from("VMCP reuse not yet implemented"))
            }
        } else {
            tracing::error!(
                vmcp_name = %name,
                status = %status,
                "VMCP creation returned error status"
            );
            Err(FilterError::from(format!("VMCP creation failed with status {}", status)))
        }
    }

    /// Fetch MCP tools from a VMCP server via SSE/MCP protocol.
    async fn fetch_mcp_tools(
        &self,
        vmcp_port: u16,
        env_id: &str,
    ) -> Result<Vec<serde_json::Value>, FilterError> {
        use std::collections::HashMap;
        use mcp_client::{ClientCapabilities, ClientInfo, McpClient, McpClientTrait, McpService, Transport};
        use mcp_client::transport::SseTransport;

        let sse_url = format!("http://localhost:{}/sse", vmcp_port);
        
        tracing::debug!(
            vmcp_port = %vmcp_port,
            env_id = %env_id,
            sse_url = %sse_url,
            "fetching MCP tools via SSE"
        );

        // Create SSE transport with empty environment
        let env = HashMap::new();
        let transport = SseTransport::new(&sse_url, env);
        
        // Start the transport to get a handle
        let transport_handle = transport.start()
            .await
            .map_err(|e| -> FilterError {
                tracing::error!(
                    vmcp_port = %vmcp_port,
                    error = %e,
                    "failed to start SSE transport"
                );
                FilterError::from(format!("SSE transport start failed: {e}"))
            })?;
        
        // Create MCP service from transport handle
        let service = McpService::new(transport_handle);
        
        // Create MCP client
        let mut client = McpClient::new(service);
        
        // Prepare client info
        let client_info = ClientInfo {
            name: "praxis-vmcp-manager".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        };
        
        let capabilities = ClientCapabilities::default();
        
        // Initialize the connection with timeout
        let init_result = tokio::time::timeout(self.timeout, client.initialize(client_info, capabilities))
            .await
            .map_err(|_| -> FilterError {
                tracing::error!(vmcp_port = %vmcp_port, "MCP initialization timeout");
                FilterError::from("MCP initialization timeout")
            })?
            .map_err(|e| -> FilterError {
                tracing::error!(
                    vmcp_port = %vmcp_port,
                    error = %e,
                    "MCP initialization failed"
                );
                FilterError::from(format!("MCP initialization failed: {e}"))
            })?;
        
        tracing::debug!(
            vmcp_port = %vmcp_port,
            server_name = %init_result.server_info.name,
            server_version = %init_result.server_info.version,
            "MCP client initialized"
        );
        
        // List available tools
        let tools_result = tokio::time::timeout(self.timeout, client.list_tools(None))
            .await
            .map_err(|_| -> FilterError {
                tracing::error!(vmcp_port = %vmcp_port, "MCP list_tools timeout");
                FilterError::from("MCP list_tools timeout")
            })?
            .map_err(|e| -> FilterError {
                tracing::error!(
                    vmcp_port = %vmcp_port,
                    error = %e,
                    "failed to list MCP tools"
                );
                FilterError::from(format!("MCP list_tools failed: {e}"))
            })?;

        // Convert MCP tools to JSON values for storage
        let tools: Vec<serde_json::Value> = tools_result.tools
            .into_iter()
            .map(|tool| {
                serde_json::json!({
                    "name": tool.name,
                    "description": tool.description,
                    "input_schema": tool.input_schema,
                })
            })
            .collect();

        tracing::info!(
            vmcp_port = %vmcp_port,
            tool_count = %tools.len(),
            "fetched MCP tools, storing in metadata"
        );

        Ok(tools)
    }
}

#[async_trait]
impl HttpFilter for VmcpManagerFilter {
    fn name(&self) -> &'static str {
        "vmcp_manager"
    }

    fn request_body_access(&self) -> BodyAccess {
        BodyAccess::ReadOnly
    }

    fn request_body_mode(&self) -> BodyMode {
        BodyMode::StreamBuffer {
            max_bytes: Some(10_485_760), // 10MB
        }
    }

    async fn on_request_body(
        &self,
        ctx: &mut HttpFilterContext<'_>,
        _body: &mut Option<Bytes>,
        end_of_stream: bool,
    ) -> Result<FilterAction, FilterError> {
        tracing::info!("vmcp_manager: on_request_body called, end_of_stream={}", end_of_stream);
        
        // Wait for complete body before processing
        if !end_of_stream {
            return Ok(FilterAction::Continue);
        }

        tracing::info!("vmcp_manager: processing complete body");

        // Get env_id from metadata (set by context_extractor in body phase)
        let env_id = ctx.filter_metadata
            .get("env_id")
            .ok_or_else(|| {
                tracing::error!("env_id not found in filter metadata");
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "env_id is required but not found in context",
                ))
            })?
            .clone();

        // Get skill_uuid from metadata (optional)
        let skill_uuid = ctx.filter_metadata.get("skill_uuid").map(String::as_str);

        // Generate VMCP server name
        let vmcp_name = self.generate_vmcp_name(&env_id);

        tracing::info!(
            env_id = %env_id,
            vmcp_name = %vmcp_name,
            skill_uuid = ?skill_uuid,
            "creating VMCP server"
        );

        // Create VMCP server
        let vmcp = match self.create_vmcp_server(&vmcp_name, skill_uuid, &env_id).await {
            Ok(vmcp) => {
                tracing::info!(
                    vmcp_name = %vmcp_name,
                    vmcp_uuid = %vmcp.uuid,
                    vmcp_port = %vmcp.port,
                    "VMCP server created successfully"
                );

                // Store VMCP details in metadata
                ctx.filter_metadata.insert("vmcp_uuid".to_string(), vmcp.uuid.clone());
                ctx.filter_metadata.insert("vmcp_name".to_string(), vmcp.name.clone());
                ctx.filter_metadata.insert("vmcp_port".to_string(), vmcp.port.to_string());
                
                // Store tool count if available
                if let Some(ref tools) = vmcp.runtime_tools {
                    if let Some(tools_array) = tools.as_array() {
                        ctx.filter_metadata.insert(
                            "vmcp_tools_count".to_string(),
                            tools_array.len().to_string()
                        );
                    }
                }

                vmcp
            }
            Err(e) => {
                tracing::error!(
                    vmcp_name = %vmcp_name,
                    error = %e,
                    "failed to create VMCP server"
                );
                return Err(e);
            }
        };

        // Fetch MCP tools from the VMCP server
        match self.fetch_mcp_tools(vmcp.port, &env_id).await {
            Ok(tools) => {
                tracing::info!(
                    vmcp_port = %vmcp.port,
                    tool_count = %tools.len(),
                    "successfully fetched MCP tools"
                );

                // Store tools in metadata for mcp_tools_enricher
                let tools_json = serde_json::to_string(&tools)
                    .map_err(|e| -> FilterError {
                        tracing::error!(error = %e, "failed to serialize MCP tools");
                        FilterError::from(format!("failed to serialize tools: {e}"))
                    })?;
                
                ctx.filter_metadata.insert("mcp_tools".to_string(), tools_json);

                Ok(FilterAction::Continue)
            }
            Err(e) => {
                tracing::error!(
                    vmcp_port = %vmcp.port,
                    error = %e,
                    "failed to fetch MCP tools"
                );
                // Continue anyway - tools enrichment will be skipped
                Ok(FilterAction::Continue)
            }
        }
    }

    async fn on_request(&self, _ctx: &mut HttpFilterContext<'_>) -> Result<FilterAction, FilterError> {
        // VMCP creation and tool fetching now happens in on_request_body
        Ok(FilterAction::Continue)
    }
}

