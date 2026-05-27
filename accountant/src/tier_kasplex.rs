//! Cached HTTP [`TierClassifier`] backed by Kasplex (KRC-721 +
//! KRC-20) indexers.
//!
//! Elite criterion (per [ADR-0012]):
//! 1. **OR**: the wallet owns ≥ 1 token in the `NACHO` KRC-721
//!    collection, looked up via
//!    `GET {nft_base}/api/v1/krc721/mainnet/address/{addr}/NACHO`.
//! 2. **OR**: the wallet's NACHO KRC-20 balance + locked
//!    holdings ≥ 100,000,000 NACHO at the token's 8-decimal
//!    precision (≥ 10^16 base units), looked up via
//!    `GET {krc20_base}/v1/krc20/address/{addr}/token/NACHO`.
//!
//! Either condition individually qualifies the wallet as Elite.
//! Both endpoints are queried in parallel for latency.
//!
//! ## Safety fallback
//!
//! On *any* error (network, deserialisation, non-200, timeout)
//! the classifier returns `Standard`. Per ADR-0012 the safe
//! direction is to **under-rebate** rather than over-rebate when
//! an upstream is degraded: a pool that paid out 33% when the
//! miner deserved 100% is a customer-service issue; a pool that
//! paid out 100% when the miner deserved 33% is a treasury leak.
//!
//! ## Caching
//!
//! In-process TTL cache keyed by wallet address. Default 5-minute
//! TTL — short enough that an operator who just NACHO-tipped a
//! miner sees the upgrade within one allocation cycle, long
//! enough that the kasplex endpoints aren't hit on every matured
//! block in steady state.
//!
//! [ADR-0012]: ../../../docs/decisions/0012-fee-model-and-tier-classification.md

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use katpool_domain::WalletAddress;
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use tokio::sync::Mutex;
use tracing::{debug, warn};

use crate::config::WalletTier;
use crate::tier::{ClassifierError, TierClassifier};

/// Default KRC-721 indexer base URL (mainnet).
pub const DEFAULT_NFT_BASE: &str = "https://krc721.kat.foundation";

/// Default KRC-20 indexer base URL (mainnet).
pub const DEFAULT_KRC20_BASE: &str = "https://api.kasplex.org";

/// Default NACHO collection ticker (case-sensitive on kasplex).
pub const DEFAULT_NACHO_TICKER: &str = "NACHO";

/// Default minimum KRC-20 NACHO base-unit holding for Elite tier:
/// `100_000_000 NACHO × 10^8 base-units-per-NACHO = 10^16`.
pub const DEFAULT_ELITE_KRC20_THRESHOLD: u128 = 10_000_000_000_000_000;

/// Default cache TTL (5 minutes).
pub const DEFAULT_TTL: Duration = Duration::from_secs(5 * 60);

/// Default per-request HTTP timeout.
pub const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(5);

/// Configuration for [`KasplexTierClassifier`].
///
/// All fields have sensible defaults via [`Self::default`];
/// production code should construct via `Default::default()` and
/// tests override the base URLs to point at a wiremock server.
#[derive(Debug, Clone)]
pub struct KasplexConfig {
    /// Base URL for the KRC-721 indexer (no trailing slash).
    pub nft_base: String,
    /// Base URL for the KRC-20 indexer (no trailing slash).
    pub krc20_base: String,
    /// Collection ticker — case-sensitive on kasplex.
    pub nft_ticker: String,
    /// KRC-20 ticker (same as collection ticker in production).
    pub krc20_ticker: String,
    /// Minimum KRC-20 base-unit holding for Elite tier.
    pub elite_krc20_threshold_base_units: u128,
    /// Cache TTL.
    pub ttl: Duration,
    /// HTTP request timeout.
    pub http_timeout: Duration,
}

impl Default for KasplexConfig {
    fn default() -> Self {
        Self {
            nft_base: DEFAULT_NFT_BASE.to_owned(),
            krc20_base: DEFAULT_KRC20_BASE.to_owned(),
            nft_ticker: DEFAULT_NACHO_TICKER.to_owned(),
            krc20_ticker: DEFAULT_NACHO_TICKER.to_owned(),
            elite_krc20_threshold_base_units: DEFAULT_ELITE_KRC20_THRESHOLD,
            ttl: DEFAULT_TTL,
            http_timeout: DEFAULT_HTTP_TIMEOUT,
        }
    }
}

/// One cache entry.
#[derive(Debug, Clone, Copy)]
struct CacheEntry {
    tier: WalletTier,
    expires_at: Instant,
}

/// HTTP-backed tier classifier with in-process TTL cache.
#[derive(Debug, Clone)]
pub struct KasplexTierClassifier {
    cfg: KasplexConfig,
    http: Client,
    cache: Arc<Mutex<HashMap<String, CacheEntry>>>,
}

impl KasplexTierClassifier {
    /// Construct a classifier with the given config. Builds an
    /// owned `reqwest::Client` with the config's timeout.
    pub fn new(cfg: KasplexConfig) -> Result<Self, ClassifierError> {
        let http = Client::builder()
            .timeout(cfg.http_timeout)
            // Aggressive connect-timeout: an unreachable kasplex
            // shouldn't stall block-maturity allocation.
            .connect_timeout(Duration::from_secs(2))
            // Strip default User-Agent leak.
            .user_agent(concat!("katpool-accountant/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| ClassifierError::Upstream(format!("reqwest build: {e}")))?;
        Ok(Self {
            cfg,
            http,
            cache: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Cache size (testing helper).
    #[must_use]
    pub async fn cache_len(&self) -> usize {
        self.cache.lock().await.len()
    }

    /// Drop every cached entry (testing + ops helper).
    pub async fn clear_cache(&self) {
        self.cache.lock().await.clear();
    }

    async fn cached(&self, key: &str) -> Option<WalletTier> {
        let now = Instant::now();
        let mut cache = self.cache.lock().await;
        if let Some(entry) = cache.get(key) {
            if entry.expires_at > now {
                return Some(entry.tier);
            }
            cache.remove(key);
        }
        None
    }

    async fn store(&self, key: String, tier: WalletTier) {
        let entry = CacheEntry {
            tier,
            expires_at: Instant::now() + self.cfg.ttl,
        };
        self.cache.lock().await.insert(key, entry);
    }

    /// Returns `Ok(true)` iff the wallet owns ≥ 1 NACHO NFT.
    /// `Ok(false)` for legitimate empty results; `Err(...)` for
    /// any failure mode the caller should treat as
    /// classification-degraded.
    async fn fetch_nft(&self, wallet: &WalletAddress) -> Result<bool, ClassifierError> {
        let url = format!(
            "{}/api/v1/krc721/mainnet/address/{}/{}",
            self.cfg.nft_base,
            wallet.as_str(),
            self.cfg.nft_ticker
        );
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| ClassifierError::Upstream(format!("nft GET {url}: {e}")))?;
        if resp.status() != StatusCode::OK {
            return Err(ClassifierError::Upstream(format!(
                "nft endpoint returned {} for {url}",
                resp.status()
            )));
        }
        let body: KrcEnvelope<NftRow> = resp
            .json()
            .await
            .map_err(|e| ClassifierError::Malformed(format!("nft JSON: {e}")))?;
        Ok(!body.result.is_empty())
    }

    /// Returns `Ok(true)` iff the wallet's NACHO KRC-20 holdings
    /// (`balance + locked`) meet the configured Elite threshold.
    /// Empty result rows ⇒ wallet has no NACHO balance row ⇒
    /// not elite by this dimension.
    async fn fetch_krc20(&self, wallet: &WalletAddress) -> Result<bool, ClassifierError> {
        let url = format!(
            "{}/v1/krc20/address/{}/token/{}",
            self.cfg.krc20_base,
            wallet.as_str(),
            self.cfg.krc20_ticker
        );
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| ClassifierError::Upstream(format!("krc20 GET {url}: {e}")))?;
        if resp.status() != StatusCode::OK {
            return Err(ClassifierError::Upstream(format!(
                "krc20 endpoint returned {} for {url}",
                resp.status()
            )));
        }
        let body: KrcEnvelope<Krc20Row> = resp
            .json()
            .await
            .map_err(|e| ClassifierError::Malformed(format!("krc20 JSON: {e}")))?;
        // Sum balance + locked across every returned row. Kasplex
        // returns at most one row per (address, ticker) in practice,
        // but the schema is plural so we sum to be safe.
        let mut total: u128 = 0;
        for row in body.result {
            let bal = parse_base_units(&row.balance)
                .map_err(|e| ClassifierError::Malformed(format!("balance: {e}")))?;
            let locked = parse_base_units(&row.locked)
                .map_err(|e| ClassifierError::Malformed(format!("locked: {e}")))?;
            total = total
                .checked_add(bal)
                .and_then(|t| t.checked_add(locked))
                .ok_or_else(|| {
                    ClassifierError::Malformed("balance + locked overflows u128".to_owned())
                })?;
            if total >= self.cfg.elite_krc20_threshold_base_units {
                return Ok(true);
            }
        }
        Ok(total >= self.cfg.elite_krc20_threshold_base_units)
    }
}

#[async_trait]
impl TierClassifier for KasplexTierClassifier {
    async fn classify(&self, wallet: &WalletAddress) -> Result<WalletTier, ClassifierError> {
        let key = wallet.as_str().to_owned();
        if let Some(cached) = self.cached(&key).await {
            debug!(wallet = %wallet.as_str(), tier = cached.as_str(), "tier from cache");
            return Ok(cached);
        }

        // Query both endpoints in parallel; either-true ⇒ Elite.
        // We do NOT short-circuit on the first response because the
        // second request is already in flight; cancelling it would
        // leak a connection. Joining both completes both, then
        // takes the disjunction.
        let nft_fut = self.fetch_nft(wallet);
        let krc20_fut = self.fetch_krc20(wallet);
        let (nft_res, krc20_res) = tokio::join!(nft_fut, krc20_fut);

        let tier = match (nft_res, krc20_res) {
            (Ok(true), _) | (_, Ok(true)) => WalletTier::Elite,
            (Ok(false), Ok(false)) => WalletTier::Standard,
            // At least one endpoint errored AND the successful one
            // (if any) returned false. Safe fallback: Standard.
            (nft, krc20) => {
                warn!(
                    wallet = %wallet.as_str(),
                    nft_err = ?nft.err(),
                    krc20_err = ?krc20.err(),
                    "tier classification degraded; defaulting to Standard"
                );
                WalletTier::Standard
            }
        };

        self.store(key, tier).await;
        Ok(tier)
    }
}

// ---------- response shapes ------------------------------------------

#[derive(Debug, Deserialize)]
struct KrcEnvelope<T> {
    // `message` field is intentionally not deserialised — KRC-721
    // returns "success", KRC-20 returns "successful", we don't
    // gate on either.
    result: Vec<T>,
    // `next` cursor present only when there are more pages; we
    // only need the first page for the membership / threshold
    // check, so the field is omitted.
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct NftRow {
    tick: String,
    #[serde(rename = "tokenId")]
    token_id: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Krc20Row {
    tick: String,
    balance: String,
    locked: String,
    dec: String,
}

/// Parse a base-unit string (kasplex returns numeric values as
/// strings to dodge JavaScript's safe-int limit). Rejects empty,
/// leading-zeros (other than literal `"0"`), or non-digit input.
fn parse_base_units(s: &str) -> Result<u128, String> {
    if s.is_empty() {
        return Err("empty".to_owned());
    }
    if s != "0" && s.starts_with('0') {
        return Err(format!("leading zeros in `{s}`"));
    }
    if !s.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("non-digit in `{s}`"));
    }
    s.parse::<u128>().map_err(|e| format!("`{s}`: {e}"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    #[test]
    fn parse_zero() {
        assert_eq!(parse_base_units("0").unwrap(), 0);
    }

    #[test]
    fn parse_max_base_units() {
        assert_eq!(
            parse_base_units("10000000000000000").unwrap(),
            10_000_000_000_000_000
        );
    }

    #[test]
    fn rejects_empty() {
        assert!(parse_base_units("").is_err());
    }

    #[test]
    fn rejects_leading_zero() {
        assert!(parse_base_units("01").is_err());
    }

    #[test]
    fn rejects_non_digit() {
        assert!(parse_base_units("123abc").is_err());
        assert!(parse_base_units("-1").is_err());
        assert!(parse_base_units("1.5").is_err());
    }

    #[test]
    fn default_config_uses_documented_constants() {
        let c = KasplexConfig::default();
        assert_eq!(c.nft_base, DEFAULT_NFT_BASE);
        assert_eq!(c.krc20_base, DEFAULT_KRC20_BASE);
        assert_eq!(c.elite_krc20_threshold_base_units, 10_000_000_000_000_000);
        assert_eq!(c.ttl, DEFAULT_TTL);
    }
}
