// SPDX-License-Identifier: MIT
// Copyright (c) 2024 Praxis Contributors

//! Creates and manages Virtual MCP (VMCP) servers.
//!
//! This filter creates VMCP servers via the skillberry-store API,
//! providing MCP tool access for specific environments and skills.

mod config;
mod filter;

#[cfg(test)]
#[expect(clippy::allow_attributes, reason = "blanket test suppressions")]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "tests"
)]
mod tests;

pub use self::filter::VmcpManagerFilter;

