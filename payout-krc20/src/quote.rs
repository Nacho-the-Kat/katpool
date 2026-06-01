//! Floor-price quote source for the KAS→NACHO payout conversion.
//!
//! Per [ADR-0016] the NACHO floor price is fetched from `api.kaspa.com`
//! over plain HTTPS (no headless browser), behind a [`FloorPriceSource`]
//! trait so the engine and tests can substitute a fake, and wrapped in a
//! [`CircuitBreaker`] that fails the cycle **closed** (skip, never guess)
//! when the upstream is degraded.
//!
//! [ADR-0016]: ../../../docs/decisions/0016-krc20-payout-conversion-and-floor-price.md

use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use serde::Deserialize;

use crate::rebate::{FloorPrice, RebateError};

/// Default floor-price API base URL (no trailing slash).
pub const DEFAULT_QUOTE_BASE: &str = "https://api.kaspa.com";

/// Default token ticker to quote.
pub const DEFAULT_QUOTE_TICKER: &str = "NACHO";

/// Default per-request HTTP timeout.
pub const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(5);

/// Errors from fetching or interpreting a floor-price quote.
#[derive(Debug, thiserror::Error)]
pub enum QuoteError {
    /// Transport / connection / timeout failure talking to the API.
    #[error("quote upstream: {0}")]
    Upstream(String),

    /// The API returned a non-200 status.
    #[error("quote endpoint returned {0}")]
    Status(StatusCode),

    /// The response body was missing, empty, or not the expected shape.
    #[error("quote malformed: {0}")]
    Malformed(String),

    /// The quoted price could not be parsed into a [`FloorPrice`].
    #[error(transparent)]
    Price(#[from] RebateError),

    /// The circuit breaker is open; the request was not attempted.
    #[error("quote circuit open")]
    CircuitOpen,
}

/// A source of NACHO floor-price quotes.
#[async_trait]
pub trait FloorPriceSource: Send + Sync {
    /// Fetches the current floor price for `ticker`.
    async fn floor_price(&self, ticker: &str) -> Result<FloorPrice, QuoteError>;
}

/// One element of the `api.kaspa.com/api/floor-price` array response:
/// `[{"ticker":"NACHO","floor_price":0.000365}]`. `floor_price` is captured
/// as a raw JSON number (`serde_json::Number`) so it can be parsed exactly
/// from its decimal string — never through `f64`.
#[derive(Debug, Deserialize)]
struct FloorPriceRow {
    #[allow(dead_code)]
    ticker: String,
    floor_price: serde_json::Number,
}

/// Parses the floor-price API body into a [`FloorPrice`].
///
/// # Errors
///
/// Returns [`QuoteError::Malformed`] for an empty array or a price that is
/// not a plain decimal, propagating the precise [`RebateError`] otherwise.
pub fn parse_floor_price_response(body: &[u8]) -> Result<FloorPrice, QuoteError> {
    let rows: Vec<FloorPriceRow> =
        serde_json::from_slice(body).map_err(|e| QuoteError::Malformed(format!("json: {e}")))?;
    let first = rows
        .first()
        .ok_or_else(|| QuoteError::Malformed("empty array".to_owned()))?;
    // `Number::to_string` yields the canonical decimal text (e.g. "0.000365")
    // without ever going through a lossy float.
    Ok(FloorPrice::from_decimal_str(
        &first.floor_price.to_string(),
    )?)
}

/// HTTP-backed floor-price source for `api.kaspa.com`.
#[derive(Debug, Clone)]
pub struct KaspaComFloorPrice {
    base: String,
    http: Client,
}

impl KaspaComFloorPrice {
    /// Builds a client against `base` (no trailing slash) with the given
    /// request timeout.
    ///
    /// # Errors
    ///
    /// Returns [`QuoteError::Upstream`] if the `reqwest` client cannot be
    /// built.
    pub fn new(base: impl Into<String>, http_timeout: Duration) -> Result<Self, QuoteError> {
        let http = Client::builder()
            .timeout(http_timeout)
            .connect_timeout(Duration::from_secs(2))
            .user_agent(concat!("katpool-payout-krc20/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| QuoteError::Upstream(format!("reqwest build: {e}")))?;
        Ok(Self {
            base: base.into(),
            http,
        })
    }
}

impl Default for KaspaComFloorPrice {
    /// Production default: `api.kaspa.com` with the default timeout.
    ///
    /// # Panics
    ///
    /// Only if `reqwest` cannot build a client with static, valid settings,
    /// which does not happen in practice.
    fn default() -> Self {
        #[allow(clippy::expect_used)]
        Self::new(DEFAULT_QUOTE_BASE, DEFAULT_HTTP_TIMEOUT).expect("static reqwest client builds")
    }
}

#[async_trait]
impl FloorPriceSource for KaspaComFloorPrice {
    async fn floor_price(&self, ticker: &str) -> Result<FloorPrice, QuoteError> {
        let url = format!("{}/api/floor-price?ticker={ticker}", self.base);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| QuoteError::Upstream(format!("{url}: {e}")))?;
        if resp.status() != StatusCode::OK {
            return Err(QuoteError::Status(resp.status()));
        }
        let body = resp
            .bytes()
            .await
            .map_err(|e| QuoteError::Upstream(format!("body: {e}")))?;
        parse_floor_price_response(&body)
    }
}

// ---------- circuit breaker ------------------------------------------

/// Circuit-breaker state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Requests pass through; failures are counted.
    Closed,
    /// Requests short-circuit until the cooldown elapses.
    Open,
    /// A single trial request is allowed to probe recovery.
    HalfOpen,
}

/// A pure, time-injected circuit-breaker state machine.
///
/// `Closed → Open` after `failure_threshold` consecutive failures;
/// `Open → HalfOpen` once `cooldown` has elapsed; `HalfOpen → Closed` on a
/// success, or back to `Open` on a failure. Time is supplied by the caller
/// ([`Instant`]) so transitions are deterministic in tests.
#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    state: CircuitState,
    consecutive_failures: u32,
    failure_threshold: u32,
    cooldown: Duration,
    opened_at: Option<Instant>,
}

impl CircuitBreaker {
    /// Builds a closed breaker that opens after `failure_threshold`
    /// consecutive failures and probes again after `cooldown`.
    #[must_use]
    pub const fn new(failure_threshold: u32, cooldown: Duration) -> Self {
        Self {
            state: CircuitState::Closed,
            consecutive_failures: 0,
            failure_threshold,
            cooldown,
            opened_at: None,
        }
    }

    /// Current state, after applying any time-based `Open → HalfOpen`
    /// transition relative to `now`.
    #[must_use]
    pub fn state(&self, now: Instant) -> CircuitState {
        match (self.state, self.opened_at) {
            (CircuitState::Open, Some(opened)) if now.duration_since(opened) >= self.cooldown => {
                CircuitState::HalfOpen
            }
            (s, _) => s,
        }
    }

    /// Whether a request should be attempted now (i.e. the circuit is not
    /// open). Call before issuing a request.
    #[must_use]
    pub fn allows_request(&self, now: Instant) -> bool {
        self.state(now) != CircuitState::Open
    }

    /// Records a successful request: resets failures and closes the circuit.
    pub const fn on_success(&mut self) {
        self.state = CircuitState::Closed;
        self.consecutive_failures = 0;
        self.opened_at = None;
    }

    /// Records a failed request at `now`. Trips the circuit open once the
    /// consecutive-failure threshold is reached (or immediately re-opens
    /// from half-open).
    pub fn on_failure(&mut self, now: Instant) {
        if self.state(now) == CircuitState::HalfOpen {
            self.state = CircuitState::Open;
            self.opened_at = Some(now);
            return;
        }
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if self.consecutive_failures >= self.failure_threshold {
            self.state = CircuitState::Open;
            self.opened_at = Some(now);
        }
    }
}

/// A [`FloorPriceSource`] wrapped in a [`CircuitBreaker`].
///
/// While the circuit is open, [`floor_price`](FloorPriceSource::floor_price)
/// returns [`QuoteError::CircuitOpen`] without touching the upstream. The
/// breaker is behind a `Mutex` so the guarded source stays `Send + Sync`.
pub struct BreakeredSource<S: FloorPriceSource> {
    inner: S,
    breaker: tokio::sync::Mutex<CircuitBreaker>,
}

impl<S: FloorPriceSource> BreakeredSource<S> {
    /// Wraps `inner` with a fresh breaker.
    #[must_use]
    pub fn new(inner: S, breaker: CircuitBreaker) -> Self {
        Self {
            inner,
            breaker: tokio::sync::Mutex::new(breaker),
        }
    }
}

#[async_trait]
impl<S: FloorPriceSource> FloorPriceSource for BreakeredSource<S> {
    async fn floor_price(&self, ticker: &str) -> Result<FloorPrice, QuoteError> {
        let now = Instant::now();
        {
            let breaker = self.breaker.lock().await;
            if !breaker.allows_request(now) {
                return Err(QuoteError::CircuitOpen);
            }
        }
        let result = self.inner.floor_price(ticker).await;
        let mut breaker = self.breaker.lock().await;
        match &result {
            Ok(_) => breaker.on_success(),
            Err(_) => breaker.on_failure(Instant::now()),
        }
        result
    }
}
