// SPDX-License-Identifier: MIT
// Copyright (c) 2024 Praxis Contributors

//! Filter to print human messages from chat requests to console.

use async_trait::async_trait;
use bytes::Bytes;

use crate::{
    FilterAction, FilterError,
    body::{BodyAccess, BodyMode},
    filter::{HttpFilter, HttpFilterContext},
};

/// Filter that prints human messages from chat requests to console.
pub struct PrintHumanMessageFilter;

impl PrintHumanMessageFilter {
    pub fn from_config(_config: &serde_yaml::Value) -> Result<Box<dyn HttpFilter>, FilterError> {
        Ok(Box::new(Self))
    }
}

#[async_trait]
impl HttpFilter for PrintHumanMessageFilter {
    fn name(&self) -> &'static str {
        "print_human_message"
    }

    fn request_body_access(&self) -> BodyAccess {
        BodyAccess::ReadOnly
    }

    fn request_body_mode(&self) -> BodyMode {
        BodyMode::StreamBuffer {
            max_bytes: Some(1_048_576), // 1MB
        }
    }

    async fn on_request(&self, _ctx: &mut HttpFilterContext<'_>) -> Result<FilterAction, FilterError> {
        Ok(FilterAction::Continue)
    }

    async fn on_request_body(
        &self,
        _ctx: &mut HttpFilterContext<'_>,
        body: &mut Option<Bytes>,
        end_of_stream: bool,
    ) -> Result<FilterAction, FilterError> {
        if !end_of_stream {
            return Ok(FilterAction::Continue);
        }

        let Some(chunk) = body.as_ref() else {
            return Ok(FilterAction::Continue);
        };

        // Parse JSON and extract user messages
        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(chunk) {
            if let Some(messages) = value.get("messages").and_then(|m| m.as_array()) {
                for msg in messages {
                    if msg.get("role").and_then(|r| r.as_str()) == Some("user") {
                        if let Some(content) = msg.get("content").and_then(|c| c.as_str()) {
                            println!("\n{}", "=".repeat(60));
                            println!("[HUMAN MESSAGE] {}", content);
                            println!("{}\n", "=".repeat(60));
                        }
                    }
                }
            }
        }

        Ok(FilterAction::Continue)
    }
}

// Made with Bob
