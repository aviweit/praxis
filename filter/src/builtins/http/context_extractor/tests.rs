// SPDX-License-Identifier: MIT
// Copyright (c) 2024 Praxis Contributors

//! Unit tests for the context extractor filter.

use http::HeaderMap;

use super::ContextExtractorFilter;
use crate::{
    FilterAction,
    filter::{HttpFilter, HttpFilterContext},
    test_util::mock_context,
};

#[tokio::test]
async fn test_extract_single_header() {
    let yaml = serde_yaml::from_str(
        r#"
headers:
  - name: x-user-id
    metadata_key: user_id
"#,
    )
    .unwrap();

    let filter = ContextExtractorFilter::from_config(&yaml).unwrap();
    let mut ctx = mock_context();
    
    // Add header
    ctx.request_headers.insert("x-user-id", "user123".parse().unwrap());

    let result = filter.on_request(&mut ctx).await.unwrap();
    assert_eq!(result, FilterAction::Continue);
    assert_eq!(ctx.filter_metadata.get("user_id"), Some(&"user123".to_string()));
}

#[tokio::test]
async fn test_extract_multiple_headers() {
    let yaml = serde_yaml::from_str(
        r#"
headers:
  - name: x-user-id
    metadata_key: user_id
  - name: x-env-id
    metadata_key: env_id
"#,
    )
    .unwrap();

    let filter = ContextExtractorFilter::from_config(&yaml).unwrap();
    let mut ctx = mock_context();
    
    ctx.request_headers.insert("x-user-id", "user123".parse().unwrap());
    ctx.request_headers.insert("x-env-id", "prod".parse().unwrap());

    let result = filter.on_request(&mut ctx).await.unwrap();
    assert_eq!(result, FilterAction::Continue);
    assert_eq!(ctx.filter_metadata.get("user_id"), Some(&"user123".to_string()));
    assert_eq!(ctx.filter_metadata.get("env_id"), Some(&"prod".to_string()));
}

#[tokio::test]
async fn test_missing_header_with_default() {
    let yaml = serde_yaml::from_str(
        r#"
headers:
  - name: x-user-id
    metadata_key: user_id
    default: "anonymous"
"#,
    )
    .unwrap();

    let filter = ContextExtractorFilter::from_config(&yaml).unwrap();
    let mut ctx = mock_context();
    
    // Don't add the header

    let result = filter.on_request(&mut ctx).await.unwrap();
    assert_eq!(result, FilterAction::Continue);
    assert_eq!(ctx.filter_metadata.get("user_id"), Some(&"anonymous".to_string()));
}

#[tokio::test]
async fn test_missing_optional_header() {
    let yaml = serde_yaml::from_str(
        r#"
headers:
  - name: x-user-id
    metadata_key: user_id
    required: false
"#,
    )
    .unwrap();

    let filter = ContextExtractorFilter::from_config(&yaml).unwrap();
    let mut ctx = mock_context();
    
    // Don't add the header

    let result = filter.on_request(&mut ctx).await.unwrap();
    assert_eq!(result, FilterAction::Continue);
    assert_eq!(ctx.filter_metadata.get("user_id"), None);
}

#[tokio::test]
async fn test_missing_required_header() {
    let yaml = serde_yaml::from_str(
        r#"
headers:
  - name: x-user-id
    metadata_key: user_id
    required: true
"#,
    )
    .unwrap();

    let filter = ContextExtractorFilter::from_config(&yaml).unwrap();
    let mut ctx = mock_context();
    
    // Don't add the header

    let result = filter.on_request(&mut ctx).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Required header"));
}

#[tokio::test]
async fn test_max_length_validation() {
    let yaml = serde_yaml::from_str(
        r#"
headers:
  - name: x-user-id
    metadata_key: user_id
    max_length: 5
"#,
    )
    .unwrap();

    let filter = ContextExtractorFilter::from_config(&yaml).unwrap();
    let mut ctx = mock_context();
    
    // Add header with value exceeding max length
    ctx.request_headers.insert("x-user-id", "toolong123".parse().unwrap());

    let result = filter.on_request(&mut ctx).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("exceeds maximum length"));
}

#[tokio::test]
async fn test_pattern_validation() {
    let yaml = serde_yaml::from_str(
        r#"
headers:
  - name: x-user-id
    metadata_key: user_id
    pattern: "^[a-z]+$"
"#,
    )
    .unwrap();

    let filter = ContextExtractorFilter::from_config(&yaml).unwrap();
    let mut ctx = mock_context();
    
    // Add header with value not matching pattern
    ctx.request_headers.insert("x-user-id", "User123".parse().unwrap());

    let result = filter.on_request(&mut ctx).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("does not match required pattern"));
}

#[tokio::test]
async fn test_global_validation() {
    let yaml = serde_yaml::from_str(
        r#"
headers:
  - name: x-user-id
    metadata_key: user_id
validation:
  max_length: 10
  pattern: "^[a-zA-Z0-9]+$"
"#,
    )
    .unwrap();

    let filter = ContextExtractorFilter::from_config(&yaml).unwrap();
    let mut ctx = mock_context();
    
    // Test valid value
    ctx.request_headers.insert("x-user-id", "user123".parse().unwrap());
    let result = filter.on_request(&mut ctx).await.unwrap();
    assert_eq!(result, FilterAction::Continue);
    
    // Test invalid value (contains special char)
    let mut ctx2 = mock_context();
    ctx2.request_headers.insert("x-user-id", "user@123".parse().unwrap());
    let result2 = filter.on_request(&mut ctx2).await;
    assert!(result2.is_err());
}

#[test]
fn test_empty_headers_config() {
    let yaml = serde_yaml::from_str(
        r#"
headers: []
"#,
    )
    .unwrap();

    let result = ContextExtractorFilter::from_config(&yaml);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("must not be empty"));
}

#[test]
fn test_invalid_header_name() {
    let yaml = serde_yaml::from_str(
        r#"
headers:
  - name: "invalid header name"
    metadata_key: test
"#,
    )
    .unwrap();

    let result = ContextExtractorFilter::from_config(&yaml);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("invalid header name"));
}

#[test]
fn test_invalid_regex_pattern() {
    let yaml = serde_yaml::from_str(
        r#"
headers:
  - name: x-test
    metadata_key: test
    pattern: "[invalid("
"#,
    )
    .unwrap();

    let result = ContextExtractorFilter::from_config(&yaml);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("invalid regex pattern"));
}

