// SPDX-License-Identifier: MIT
// Copyright (c) 2024 Praxis Contributors

//! [`ContextExtractorFilter`] implementation and `HttpFilter` trait impl.

use async_trait::async_trait;
use bytes::Bytes;
use regex::Regex;

use super::config::{ContextExtractorConfig, HeaderExtractionRule, ValidationRules};
use crate::{
    FilterAction, FilterError,
    body::{BodyAccess, BodyMode},
    factory::parse_filter_config,
    filter::{HttpFilter, HttpFilterContext},
};

// -----------------------------------------------------------------------------
// CompiledRule
// -----------------------------------------------------------------------------

/// A header extraction rule with pre-compiled regex pattern.
struct CompiledRule {
    /// Original rule configuration.
    rule: HeaderExtractionRule,
    
    /// Compiled regex pattern (if specified).
    pattern: Option<Regex>,
}

// -----------------------------------------------------------------------------
// ContextExtractorFilter
// -----------------------------------------------------------------------------

/// Extracts context metadata from HTTP headers.
///
/// This filter reads specified HTTP headers from incoming requests
/// and stores their values in the filter context metadata map.
/// Downstream filters can then access these values to make routing
/// decisions, enrich requests, or perform validation.
///
/// # Features
///
/// - **Header extraction**: Maps HTTP headers to metadata keys
/// - **Default values**: Provides fallbacks for missing headers
/// - **Validation**: Enforces length limits and regex patterns
/// - **Required headers**: Returns 400 if required headers are missing
///
/// # YAML configuration
///
/// ```yaml
/// filter: context_extractor
/// headers:
///   - name: skillberry-context-env-id
///     metadata_key: env_id
///     default: "default-env"
///     required: false
///   - name: skillberry-context-user-id
///     metadata_key: user_id
///     required: true
///     max_length: 128
///     pattern: "^[a-zA-Z0-9_-]+$"
/// validation:
///   max_length: 256
///   pattern: "^[a-zA-Z0-9_-]+$"
/// ```
///
/// # Example
///
/// ```
/// use praxis_filter::ContextExtractorFilter;
///
/// let yaml: serde_yaml::Value = serde_yaml::from_str(
///     r#"
/// headers:
///   - name: x-user-id
///     metadata_key: user_id
///     default: "anonymous"
/// "#,
/// )
/// .unwrap();
/// let filter = ContextExtractorFilter::from_config(&yaml).unwrap();
/// assert_eq!(filter.name(), "context_extractor");
/// ```
pub struct ContextExtractorFilter {
    /// Compiled extraction rules.
    rules: Vec<CompiledRule>,
    
    /// Global validation rules.
    global_validation: Option<ValidationRules>,
}

impl ContextExtractorFilter {
    /// Create from YAML config.
    ///
    /// Compiles all regex patterns at construction time for
    /// efficient per-request validation.
    ///
    /// # Errors
    ///
    /// Returns [`FilterError`] if:
    /// - `headers` list is empty
    /// - A regex pattern is invalid
    /// - A header name is invalid
    ///
    /// [`FilterError`]: crate::FilterError
    pub fn from_config(config: &serde_yaml::Value) -> Result<Box<dyn HttpFilter>, FilterError> {
        let cfg: ContextExtractorConfig = parse_filter_config("context_extractor", config)?;

        if cfg.headers.is_empty() {
            return Err("context_extractor: 'headers' must not be empty".into());
        }

        let mut rules = Vec::with_capacity(cfg.headers.len());

        for rule_cfg in cfg.headers {
            // Validate header name
            http::HeaderName::from_bytes(rule_cfg.name.as_bytes()).map_err(|e| -> FilterError {
                format!(
                    "context_extractor: invalid header name '{}': {e}",
                    rule_cfg.name
                )
                .into()
            })?;

            // Compile per-rule pattern if specified
            let pattern = if let Some(ref pattern_str) = rule_cfg.pattern {
                Some(Regex::new(pattern_str).map_err(|e| -> FilterError {
                    format!(
                        "context_extractor: invalid regex pattern '{}' for header '{}': {e}",
                        pattern_str, rule_cfg.name
                    )
                    .into()
                })?)
            } else {
                None
            };

            rules.push(CompiledRule {
                rule: rule_cfg,
                pattern,
            });
        }

        // Compile global validation pattern if specified
        let global_validation = if let Some(ref val) = cfg.validation {
            if let Some(ref pattern_str) = val.pattern {
                // Validate the pattern compiles
                Regex::new(pattern_str).map_err(|e| -> FilterError {
                    format!(
                        "context_extractor: invalid global validation pattern '{}': {e}",
                        pattern_str
                    )
                    .into()
                })?;
            }
            Some(val.clone())
        } else {
            None
        };

        Ok(Box::new(Self {
            rules,
            global_validation,
        }))
    }
}

#[async_trait]
impl HttpFilter for ContextExtractorFilter {
    fn name(&self) -> &'static str {
        "context_extractor"
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
        tracing::info!("context_extractor: on_request_body called, end_of_stream={}", end_of_stream);
        
        // Wait for complete body before extracting context
        if !end_of_stream {
            return Ok(FilterAction::Continue);
        }

        tracing::info!("context_extractor: processing complete body, extracting headers");

        // Extract headers and store in metadata
        let headers = &ctx.request.headers;

        for compiled_rule in &self.rules {
            let rule = &compiled_rule.rule;
            
            // Extract header value (case-insensitive lookup)
            let header_value = headers
                .get(&rule.name)
                .and_then(|v| v.to_str().ok())
                .map(String::from);

            let value = match header_value {
                Some(v) => v,
                None => {
                    // Header is missing
                    if let Some(ref default) = rule.default {
                        // Use default value
                        tracing::debug!(
                            header = %rule.name,
                            metadata_key = %rule.metadata_key,
                            default = %default,
                            "header missing, using default"
                        );
                        default.clone()
                    } else if rule.required {
                        // Required header is missing and no default
                        tracing::warn!(
                            header = %rule.name,
                            "required header missing"
                        );
                        return Err(Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            format!(
                            "Required header '{}' is missing",
                            rule.name
                            ),
                        )));
                    } else {
                        // Optional header is missing, skip
                        tracing::debug!(
                            header = %rule.name,
                            "optional header missing, skipping"
                        );
                        continue;
                    }
                }
            };

            // Validate value
            validate_value(&value, rule, &compiled_rule.pattern, &self.global_validation)?;

            // Store in metadata
            tracing::debug!(
                header = %rule.name,
                metadata_key = %rule.metadata_key,
                value = %value,
                "extracted header to metadata"
            );
            ctx.filter_metadata.insert(rule.metadata_key.clone(), value);
        }

        Ok(FilterAction::Continue)
    }

    async fn on_request(&self, _ctx: &mut HttpFilterContext<'_>) -> Result<FilterAction, FilterError> {
        // Context extraction now happens in on_request_body
        Ok(FilterAction::Continue)
    }
}

// -----------------------------------------------------------------------------
// Validation
// -----------------------------------------------------------------------------

/// Validate an extracted header value against all applicable rules.
fn validate_value(
    value: &str,
    rule: &HeaderExtractionRule,
    compiled_pattern: &Option<Regex>,
    global_validation: &Option<ValidationRules>,
) -> Result<(), FilterError> {
    // Check per-rule max length
    if let Some(max_len) = rule.max_length {
        if value.len() > max_len {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                "Header '{}' value exceeds maximum length of {} (got {})",
                rule.name,
                max_len,
                value.len()
                ),
            )));
        }
    }

    // Check global max length
    if let Some(global_val) = global_validation {
        if let Some(max_len) = global_val.max_length {
            if value.len() > max_len {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "Header '{}' value exceeds global maximum length of {} (got {})",
                        rule.name,
                        max_len,
                        value.len()
                    ),
                )));
            }
        }
    }

    // Check per-rule pattern
    if let Some(pattern) = compiled_pattern {
        if !pattern.is_match(value) {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                "Header '{}' value does not match required pattern",
                rule.name
                ),
            )));
        }
    }

    // Check global pattern
    if let Some(global_val) = global_validation {
        if let Some(pattern_str) = &global_val.pattern {
            // Re-compile for validation (cached in production via lazy_static if needed)
            let pattern = Regex::new(pattern_str).map_err(|e| -> FilterError {
                format!("Failed to compile global pattern: {e}").into()
            })?;
            if !pattern.is_match(value) {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                    "Header '{}' value does not match global validation pattern",
                    rule.name
                    ),
                )));
            }
        }
    }

    Ok(())
}

