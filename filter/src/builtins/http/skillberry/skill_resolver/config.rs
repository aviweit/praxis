// SPDX-License-Identifier: MIT
// Copyright (c) 2024 Praxis Contributors

//! Configuration types for the skill resolver filter.

use serde::Deserialize;

// -----------------------------------------------------------------------------
// SkillResolverConfig
// -----------------------------------------------------------------------------

/// Configuration for the skill resolver filter.
///
/// Resolves skill UUIDs from environment variables, either directly
/// via SKILL_UUID or by looking up a skill name via the skillberry-store API.
///
/// ```yaml
/// filter: skill_resolver
/// store_base_url: "http://localhost:8000"
/// skill_uuid_env: "SKILL_UUID"
/// skill_name_env: "SKILL_NAME"
/// timeout_ms: 5000
/// ```
#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub(super) struct SkillResolverConfig {
    /// Base URL of the skillberry-store service.
    /// Example: "http://localhost:8000"
    pub store_base_url: String,

    /// Environment variable name containing the skill UUID.
    /// If set, this takes priority over skill_name_env.
    #[serde(default = "default_skill_uuid_env")]
    pub skill_uuid_env: String,

    /// Environment variable name containing the skill name.
    /// If set and skill_uuid_env is not set, will lookup via API.
    #[serde(default = "default_skill_name_env")]
    pub skill_name_env: String,

    /// HTTP request timeout in milliseconds.
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_skill_uuid_env() -> String {
    "SKILL_UUID".to_string()
}

fn default_skill_name_env() -> String {
    "SKILL_NAME".to_string()
}

fn default_timeout_ms() -> u64 {
    5000
}

