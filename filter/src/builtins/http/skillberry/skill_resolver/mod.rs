// SPDX-License-Identifier: MIT
// Copyright (c) 2024 Praxis Contributors

//! Resolves skill UUIDs from environment variables.
//!
//! This filter reads SKILL_UUID or SKILL_NAME from environment variables
//! and resolves them to a skill UUID via the skillberry-store API.

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

pub use self::filter::SkillResolverFilter;

