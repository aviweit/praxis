// SPDX-License-Identifier: MIT
// Copyright (c) 2024 Praxis Contributors

//! Unit tests for the VMCP manager filter.

use super::VmcpManagerFilter;
use crate::filter::HttpFilter;

#[test]
fn test_filter_creation() {
    let yaml = serde_yaml::from_str(
        r#"
store_base_url: "http://localhost:8000"
vmcp_name_template: "vmcp-{env_id}"
always_create: true
timeout_ms: 10000
"#,
    )
    .unwrap();

    let filter = VmcpManagerFilter::from_config(&yaml).unwrap();
    assert_eq!(filter.name(), "vmcp_manager");
}

#[test]
fn test_empty_store_url_fails() {
    let yaml = serde_yaml::from_str(
        r#"
store_base_url: ""
"#,
    )
    .unwrap();

    let result = VmcpManagerFilter::from_config(&yaml);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("must not be empty"));
}

// TODO: Add integration tests with mock HTTP server
// - Test VMCP creation with skill UUID
// - Test VMCP creation without skill UUID
// - Test missing env_id (error)
// - Test store unreachable
// - Test timeout
// - Test 409 Conflict handling

