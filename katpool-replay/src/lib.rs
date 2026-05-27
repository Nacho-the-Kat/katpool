//! Library surface for the `katpool-replay` binary.
//!
//! - [`legacy_log`] — best-effort adapter from legacy `katpool-app`
//!   monitoring-log lines to [`PoolEvent`]s.
//! - Re-exports [`accountant::replay`] primitives used by tests and
//!   the operator CLI.

pub mod legacy_log;

pub use accountant::replay::{
    assert_snapshots_equal, load_ndjson_path, load_ndjson_reader, replay_all, snapshot,
    verify_dual_replay, DbSnapshot,
};
pub use legacy_log::{LegacyParseReport, LegacyParseStats};
