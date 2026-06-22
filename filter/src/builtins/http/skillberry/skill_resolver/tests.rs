// SPDX-License-Identifier: MIT
// Copyright (c) 2024 Praxis Contributors

//! Unit tests for the skill resolver filter.

use super::SkillResolverFilter;
use crate::filter::HttpFilter;

#[test]
fn test_filter_creation() {
    let yaml = serde_yaml::from_str(
        r#"
store_base_url: "http://localhost:8000"
skill_uuid_env: "SKILL_UUID"
skill_name_env: "SKILL_NAME"
timeout_ms: 5000
"#,
    )
    .unwrap();

    let filter = SkillResolverFilter::from_config(&yaml).unwrap();
    assert_eq!(filter.name(), "skill_resolver");
}

#[test]
fn test_empty_store_url_fails() {
    let yaml = serde_yaml::from_str(
        r#"
store_base_url: ""
"#,
    )
    .unwrap();

    let result = SkillResolverFilter::from_config(&yaml);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("must not be empty"));
}

// TODO: Add integration tests with mock HTTP server
// - Test direct UUID from env
// - Test skill name lookup (success)
// - Test skill name lookup (404)
// - Test store unreachable
// - Test timeout

