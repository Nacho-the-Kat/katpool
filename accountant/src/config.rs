//! Pool fee model + tier classification configuration.
//!
//! Loaded once at startup and held read-only thereafter. The
//! topline fee is operator-tunable via env (`KATPOOL_FEE_TOPLINE_BPS`);
//! the two rebate ratios (33% for `Standard`, 100% for `Elite`)
//! are *fixed in code* per `docs/decisions/0012-fee-model-and-tier-classification.md`.
//!
//! Money math is integer basis points end-to-end. Floating-point
//! never touches a sompi figure that the pool keeps or pays out.
//!
//! ## The math, in one place
//!
//! For a wallet's gross share `G` of a block coinbase, with the
//! configured topline fee `T_bps` and the wallet's tier rebate
//! `R_bps`:
//!
//! ```text
//! fee_share     = G * T_bps / 10_000             // taken off the top
//! nacho_accrual = fee_share * R_bps / 10_000     // sompi-equivalent NACHO to miner
//! pool_fee      = fee_share - nacho_accrual      // sompi pool keeps
//! net_payout    = G - fee_share                   // KAS to miner
//! ```
//!
//! The balance equation `G = pool_fee + nacho_accrual + net_payout`
//! holds by construction and is enforced again by the schema's
//! `share_allocation_balance` CHECK.
//!
//! ## NACHO denomination
//!
//! `nacho_accrual` is **sompi**, not NACHO tokens. The KAS→NACHO
//! conversion happens only at krc-20 payout-cycle time, at the
//! prevailing market rate, so per-block accrual stays in the hard
//! asset we're mining.

use std::env;

use serde::{Deserialize, Serialize};

use crate::error::AccountantError;

/// Maximum allowed topline fee, in basis points. 1 000 bps = 10%.
/// Defensive ceiling against operator typos — a real pool will
/// never charge anything close to this.
const MAX_TOPLINE_BPS: u16 = 1_000;

/// Rebate ratio for `Standard`-tier miners: 33% of the fee.
pub const STANDARD_REBATE_BPS: u16 = 3_300;

/// Rebate ratio for `Elite`-tier miners: 100% of the fee.
pub const ELITE_REBATE_BPS: u16 = 10_000;

/// Default topline fee if `KATPOOL_FEE_TOPLINE_BPS` is unset: 75 bps = 0.75%.
pub const DEFAULT_TOPLINE_BPS: u16 = 75;

/// Pool fee model, derived from env at startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeeConfig {
    /// Topline fee in basis points. Default `DEFAULT_TOPLINE_BPS`.
    topline_bps: u16,
}

impl FeeConfig {
    /// Construct directly, validating the basis-point value. Most
    /// callers want [`Self::from_env`].
    pub const fn new(topline_bps: u16) -> Result<Self, &'static str> {
        if topline_bps > MAX_TOPLINE_BPS {
            return Err("topline_bps exceeds MAX_TOPLINE_BPS");
        }
        Ok(Self { topline_bps })
    }

    /// Load from environment. Reads `KATPOOL_FEE_TOPLINE_BPS`;
    /// defaults to `DEFAULT_TOPLINE_BPS` if unset.
    pub fn from_env() -> Result<Self, AccountantError> {
        Self::from_lookup("KATPOOL_FEE_TOPLINE_BPS", |k: &str| env::var(k))
    }

    /// Loader generic over the env-lookup function (testable
    /// without touching process state).
    ///
    /// The closure mirrors `std::env::var`: `Ok(value)` for a set
    /// var, `Err(VarError::NotPresent)` for unset, `Err(NotUnicode)`
    /// for non-UTF-8.
    pub fn from_lookup<F>(var: &str, lookup: F) -> Result<Self, AccountantError>
    where
        F: Fn(&str) -> Result<String, env::VarError>,
    {
        let topline_bps = match lookup(var) {
            Ok(s) => Self::parse_bps(var, &s)?,
            Err(env::VarError::NotPresent) => DEFAULT_TOPLINE_BPS,
            Err(env::VarError::NotUnicode(_)) => {
                return Err(AccountantError::Config(format!("{var} is not valid UTF-8")));
            }
        };
        Self::new(topline_bps)
            .map_err(|e| AccountantError::Config(format!("{var}={topline_bps}: {e}")))
    }

    /// Parse a raw env-string into a basis-point value. Pure;
    /// safe to call from tests without env mutation.
    pub fn parse_bps(var: &str, raw: &str) -> Result<u16, AccountantError> {
        raw.parse::<u16>()
            .map_err(|e| AccountantError::Config(format!("{var}='{raw}' is not a u16: {e}")))
    }

    /// Topline fee in basis points.
    #[must_use]
    pub const fn topline_bps(self) -> u16 {
        self.topline_bps
    }

    /// Rebate ratio for the given tier, in basis points.
    #[must_use]
    pub const fn rebate_bps(self, tier: WalletTier) -> u16 {
        match tier {
            WalletTier::Standard => STANDARD_REBATE_BPS,
            WalletTier::Elite => ELITE_REBATE_BPS,
        }
    }
}

/// Wallet's fee tier for one allocation. Determined at block-
/// maturity time by the [`TierClassifier`](crate::tier::TierClassifier)
/// — never derived from share-time state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "wallet_tier", rename_all = "snake_case")]
pub enum WalletTier {
    /// Default tier; 33% NACHO rebate on the topline fee.
    Standard,
    /// Holds at least one `NACHO` KRC-721 token, OR ≥ 100M NACHO
    /// (10^16 base units at 8 decimals). 100% NACHO rebate on the
    /// topline fee.
    Elite,
}

impl WalletTier {
    /// Stable lowercase string suitable for metrics labels.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Elite => "elite",
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    #[test]
    fn default_topline_is_75_bps() {
        assert_eq!(DEFAULT_TOPLINE_BPS, 75);
    }

    #[test]
    fn rebate_ratios_match_spec() {
        assert_eq!(STANDARD_REBATE_BPS, 3_300, "standard rebate = 33%");
        assert_eq!(ELITE_REBATE_BPS, 10_000, "elite rebate = 100%");
    }

    fn lookup_unset(_: &str) -> Result<String, env::VarError> {
        Err(env::VarError::NotPresent)
    }
    fn lookup_returning(value: &'static str) -> impl Fn(&str) -> Result<String, env::VarError> {
        move |_: &str| Ok(value.to_owned())
    }

    #[test]
    fn from_lookup_defaults_when_unset() {
        let cfg = FeeConfig::from_lookup("KATPOOL_FEE_TOPLINE_BPS", lookup_unset).unwrap();
        assert_eq!(cfg.topline_bps(), DEFAULT_TOPLINE_BPS);
    }

    #[test]
    fn from_lookup_accepts_valid_value() {
        let cfg =
            FeeConfig::from_lookup("KATPOOL_FEE_TOPLINE_BPS", lookup_returning("50")).unwrap();
        assert_eq!(cfg.topline_bps(), 50);
    }

    #[test]
    fn from_lookup_accepts_zero_topline() {
        // A pool may legitimately run at zero fee (community pool,
        // promotional period). The schema validates >=0 not >0;
        // we mirror that here.
        let cfg = FeeConfig::from_lookup("KATPOOL_FEE_TOPLINE_BPS", lookup_returning("0")).unwrap();
        assert_eq!(cfg.topline_bps(), 0);
    }

    #[test]
    fn from_lookup_rejects_too_large() {
        let err = FeeConfig::from_lookup("KATPOOL_FEE_TOPLINE_BPS", lookup_returning("5000"))
            .unwrap_err();
        assert!(format!("{err}").contains("MAX_TOPLINE_BPS"));
    }

    #[test]
    fn from_lookup_rejects_non_numeric() {
        assert!(
            FeeConfig::from_lookup("KATPOOL_FEE_TOPLINE_BPS", lookup_returning("abc")).is_err()
        );
    }

    #[test]
    fn from_lookup_rejects_negative_string() {
        assert!(FeeConfig::from_lookup("KATPOOL_FEE_TOPLINE_BPS", lookup_returning("-1")).is_err());
    }

    #[test]
    fn rebate_bps_by_tier() {
        let cfg = FeeConfig::new(75).unwrap();
        assert_eq!(cfg.rebate_bps(WalletTier::Standard), STANDARD_REBATE_BPS);
        assert_eq!(cfg.rebate_bps(WalletTier::Elite), ELITE_REBATE_BPS);
    }

    #[test]
    fn tier_as_str_is_stable() {
        assert_eq!(WalletTier::Standard.as_str(), "standard");
        assert_eq!(WalletTier::Elite.as_str(), "elite");
    }
}
