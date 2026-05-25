//! Treasury secret material handling.
//!
//! Implements the user-confirmed sops/age + OS-level isolation custody model
//! documented in ADR-008 and `docs/custody.md`:
//!
//! - keys at rest: sops-encrypted with age, mode 0600, owned by katpool uid
//! - keys in memory: `secrecy::Secret<[u8; 32]>`, zeroized on drop, mlocked
//! - swap: disabled at OS level; checked at boot
//! - no `Debug` impl that leaks bytes, no serialization to error messages
//!
//! Real implementation lands in Phase 4 (treasury custody for payout-kas).

#![cfg_attr(not(test), warn(missing_docs))]

/// Crate version constant.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
