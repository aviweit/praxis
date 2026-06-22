// SPDX-License-Identifier: MIT
// Copyright (c) 2024 Praxis Contributors

//! VMCP manager filter implementation.

use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::config::VmcpManagerConfig;
use crate::{
    FilterAction, FilterError,
    factory::parse_filter_config,
    filter::{HttpFilter, HttpFilterContext},
};

// -----------------------------------------------------------------------------
// Request/Response Types
// -----------------------------------------------------------------------------

/// Request body for creating a VMCP server.
#[derive(Debug, Serialize)]
struct CreateVmcpRequest {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    skill_uuid: Option<String>,
    description: String,
}

/// Response from skillberry-store POST /vmcp-servers endpoint.
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
        let url = format!("{}/vmcp-servers", self.store_base_url);
        
        let request_body = CreateVmcpRequest {
            name: name.to_string(),
            skill_uuid: skill_uuid.map(String::from),
            description: format!("VMCP server for environment {}", env_id),
        };

        tracing::debug!(
            vmcp_name = %name,
            skill_uuid = ?skill_uuid,
            env_id = %env_id,
            url = %url,
            "creating VMCP server"
        );

        let mut request = self.http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("skillberry-context-env-id", env_id)
            .json(&request_body);

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
}

#[async_trait]
impl HttpFilter for VmcpManagerFilter {
    fn name(&self) -> &'static str {
        "vmcp_manager"
    }

    async fn on_request(&self, ctx: &mut HttpFilterContext<'_>) -> Result<FilterAction, FilterError> {
        // Get env_id from metadata (required)
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
        match self.create_vmcp_server(&vmcp_name, skill_uuid, &env_id).await {
            Ok(vmcp) => {
                tracing::info!(
                    vmcp_name = %vmcp_name,
                    vmcp_uuid = %vmcp.uuid,
                    vmcp_port = %vmcp.port,
                    "VMCP server created successfully"
                );

                // Store VMCP details in metadata
                ctx.filter_metadata.insert("vmcp_uuid".to_string(), vmcp.uuid);
                ctx.filter_metadata.insert("vmcp_name".to_string(), vmcp.name);
                ctx.filter_metadata.insert("vmcp_port".to_string(), vmcp.port.to_string());
                
                // Store tool count if available
                if let Some(tools) = vmcp.runtime_tools {
                    if let Some(tools_array) = tools.as_array() {
                        ctx.filter_metadata.insert(
                            "vmcp_tools_count".to_string(),
                            tools_array.len().to_string()
                        );
                    }
                }

                Ok(FilterAction::Continue)
            }
            Err(e) => {
                tracing::error!(
                    vmcp_name = %vmcp_name,
                    error = %e,
                    "failed to create VMCP server"
                );
                Err(e)
            }
        }
    }
}

