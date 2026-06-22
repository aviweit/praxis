// SPDX-License-Identifier: MIT
// Copyright (c) 2024 Praxis Contributors

//! Skillberry integration filters.
//!
//! Filters for integrating with the Skillberry ecosystem:
//! - skill_resolver: Resolves skill UUIDs from environment variables
//! - vmcp_manager: Creates and manages Virtual MCP servers

pub(crate) mod skill_resolver;
pub(crate) mod vmcp_manager;

pub use skill_resolver::SkillResolverFilter;
pub use vmcp_manager::VmcpManagerFilter;

