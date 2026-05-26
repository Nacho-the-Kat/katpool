//! One-shot legacy-database importer for the previous-generation
//! katpool deployment.
//!
//! See [ADR-0011](../../../docs/decisions/0011-db-schema-and-migrations.md)
//! for the legacy-to-new schema mapping rationale.
//!
//! This crate ships:
//!
//! - A binary (`katpool-import-legacy`) that operators run at
//!   cutover time.
//! - Library modules ([`source`], [`transform`]) consumed by both
//!   the binary and the integration tests under `tests/`.
//!
//! The importer is **idempotent**: every transform keys writes off a
//! natural identifier and uses `ON CONFLICT` semantics, so re-runs
//! produce zero duplicates. Operators can safely re-run the importer
//! to catch up after a connection drop, a partial outage, or a
//! cutover-day surprise.

#![cfg_attr(not(test), warn(missing_docs))]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::float_arithmetic,
    )
)]

/// Crate version constant.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod reconcile;
pub mod source;
pub mod transform;
