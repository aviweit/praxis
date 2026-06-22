// SPDX-License-Identifier: MIT
// Copyright (c) 2024 Praxis Contributors

//! Deserialized YAML configuration types for the context extractor filter.

use serde::Deserialize;

// -----------------------------------------------------------------------------
// ContextExtractorConfig
// -----------------------------------------------------------------------------

/// Deserialized YAML config for the context extractor filter.
///
/// Extracts context metadata from HTTP headers and stores them
/// in the filter context for use by downstream filters.
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
/// validation:
///   max_length: 256
///   pattern: "^[a-zA-Z0-9_-]+$"
/// ```
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ContextExtractorConfig {
    /// List of headers to extract.
    pub headers: Vec<HeaderExtractionRule>,

    /// Optional validation rules applied to all extracted values.
    #[serde(default)]
    pub validation: Option<ValidationRules>,
}

// -----------------------------------------------------------------------------
// HeaderExtractionRule
// -----------------------------------------------------------------------------

/// Configuration for extracting a single header.
#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub(super) struct HeaderExtractionRule {
    /// HTTP header name to extract (case-insensitive).
    pub name: String,

    /// Metadata key to store the extracted value under.
    /// This key is used by downstream filters to access the value.
    pub metadata_key: String,

    /// Default value if header is missing.
    /// If not set and header is missing, behavior depends on `required`.
    #[serde(default)]
    pub default: Option<String>,

    /// Whether this header is required.
    /// If true and header is missing (and no default), returns 400 error.
    #[serde(default)]
    pub required: bool,

    /// Optional per-header validation pattern (regex).
    #[serde(default)]
    pub pattern: Option<String>,

    /// Optional per-header max length.
    #[serde(default)]
    pub max_length: Option<usize>,
}

// -----------------------------------------------------------------------------
// ValidationRules
// -----------------------------------------------------------------------------

/// Global validation rules applied to all extracted header values.
#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub(super) struct ValidationRules {
    /// Maximum length for any extracted value.
    /// Values exceeding this length are rejected with 400.
    #[serde(default)]
    pub max_length: Option<usize>,

    /// Regex pattern that all values must match.
    /// Values not matching are rejected with 400.
    #[serde(default)]
    pub pattern: Option<String>,
}

