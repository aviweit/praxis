// SPDX-License-Identifier: MIT
// Copyright (c) 2024 Praxis Contributors

//! Extracts context metadata from HTTP headers.
//!
//! This filter reads specified HTTP headers and stores their values
//! in the filter context metadata map for use by downstream filters.

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

pub use self::filter::ContextExtractorFilter;

