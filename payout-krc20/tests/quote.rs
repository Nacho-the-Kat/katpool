//! Deterministic tests for the floor-price quote source: the
//! time-injected circuit breaker, the response parser, and the HTTP
//! client against a wiremock server (ADR-0016).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use payout_krc20::{
    BreakeredSource, CircuitBreaker, CircuitState, FloorPrice, FloorPriceSource,
    KaspaComFloorPrice, QuoteError, parse_floor_price_response,
};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ---------- response parsing -----------------------------------------

#[test]
fn parses_kaspa_com_array_body() {
    let body = br#"[{"ticker":"NACHO","floor_price":0.000365}]"#;
    let price = parse_floor_price_response(body).expect("parse");
    assert_eq!((price.mantissa(), price.scale()), (365, 6));
}

#[test]
fn rejects_empty_array() {
    assert!(matches!(
        parse_floor_price_response(b"[]"),
        Err(QuoteError::Malformed(_))
    ));
}

#[test]
fn rejects_non_json_body() {
    assert!(matches!(
        parse_floor_price_response(b"not json"),
        Err(QuoteError::Malformed(_))
    ));
}

#[test]
fn rejects_zero_price_via_rebate_error() {
    let body = br#"[{"ticker":"NACHO","floor_price":0}]"#;
    assert!(matches!(
        parse_floor_price_response(body),
        Err(QuoteError::Price(_))
    ));
}

// ---------- circuit breaker (pure, time-injected) --------------------

#[test]
fn breaker_opens_after_threshold_then_half_opens_after_cooldown() {
    let t0 = Instant::now();
    let cooldown = Duration::from_secs(30);
    let mut cb = CircuitBreaker::new(3, cooldown);

    assert_eq!(cb.state(t0), CircuitState::Closed);
    assert!(cb.allows_request(t0));

    cb.on_failure(t0);
    cb.on_failure(t0);
    assert_eq!(
        cb.state(t0),
        CircuitState::Closed,
        "below threshold stays closed"
    );
    cb.on_failure(t0);
    assert_eq!(cb.state(t0), CircuitState::Open, "third failure trips open");
    assert!(!cb.allows_request(t0));

    // Still open just before cooldown elapses.
    assert_eq!(cb.state(t0 + Duration::from_secs(29)), CircuitState::Open);
    // Half-open once cooldown elapses.
    let t1 = t0 + cooldown;
    assert_eq!(cb.state(t1), CircuitState::HalfOpen);
    assert!(cb.allows_request(t1));
}

#[test]
fn half_open_success_closes_and_resets() {
    let t0 = Instant::now();
    let mut cb = CircuitBreaker::new(2, Duration::from_secs(10));
    cb.on_failure(t0);
    cb.on_failure(t0);
    assert_eq!(cb.state(t0), CircuitState::Open);

    cb.on_success();
    assert_eq!(cb.state(t0), CircuitState::Closed);
    // Failure counter was reset: a single failure must not re-open.
    cb.on_failure(t0);
    assert_eq!(cb.state(t0), CircuitState::Closed);
}

#[test]
fn half_open_failure_reopens_with_fresh_cooldown() {
    let t0 = Instant::now();
    let cooldown = Duration::from_secs(10);
    let mut cb = CircuitBreaker::new(1, cooldown);
    cb.on_failure(t0);
    assert_eq!(cb.state(t0), CircuitState::Open);

    let t1 = t0 + cooldown; // half-open window
    assert_eq!(cb.state(t1), CircuitState::HalfOpen);
    cb.on_failure(t1); // trial fails → reopen
    assert_eq!(cb.state(t1), CircuitState::Open);
    assert_eq!(cb.state(t1 + Duration::from_secs(9)), CircuitState::Open);
    assert_eq!(cb.state(t1 + cooldown), CircuitState::HalfOpen);
}

// ---------- breaker wrapper short-circuits ---------------------------

/// A fake source that always fails and counts how many times it was hit
/// through a shared `Arc` so the count survives being moved into the wrapper.
struct CountingFailSource {
    hits: Arc<AtomicUsize>,
}

#[async_trait]
impl FloorPriceSource for CountingFailSource {
    async fn floor_price(&self, _ticker: &str) -> Result<FloorPrice, QuoteError> {
        self.hits.fetch_add(1, Ordering::SeqCst);
        Err(QuoteError::Upstream("boom".to_owned()))
    }
}

#[tokio::test]
async fn breakered_source_short_circuits_when_open() {
    let hits = Arc::new(AtomicUsize::new(0));
    let inner = CountingFailSource {
        hits: Arc::clone(&hits),
    };
    let wrapped = BreakeredSource::new(inner, CircuitBreaker::new(2, Duration::from_secs(60)));

    // Two failures trip the breaker open.
    assert!(matches!(
        wrapped.floor_price("NACHO").await,
        Err(QuoteError::Upstream(_))
    ));
    assert!(matches!(
        wrapped.floor_price("NACHO").await,
        Err(QuoteError::Upstream(_))
    ));
    // Third call short-circuits without hitting the inner source.
    assert!(matches!(
        wrapped.floor_price("NACHO").await,
        Err(QuoteError::CircuitOpen)
    ));

    // Inner was hit exactly twice — the open call never reached upstream.
    assert_eq!(hits.load(Ordering::SeqCst), 2);
}

// ---------- HTTP client (wiremock) -----------------------------------

#[tokio::test]
async fn http_client_fetches_and_parses() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/floor-price"))
        .and(query_param("ticker", "NACHO"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"[{"ticker":"NACHO","floor_price":0.000365}]"#,
            "application/json",
        ))
        .mount(&server)
        .await;

    let client = KaspaComFloorPrice::new(server.uri(), Duration::from_secs(2)).expect("client");
    let price = client.floor_price("NACHO").await.expect("fetch");
    assert_eq!((price.mantissa(), price.scale()), (365, 6));
}

#[tokio::test]
async fn http_client_maps_non_200_to_status_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let client = KaspaComFloorPrice::new(server.uri(), Duration::from_secs(2)).expect("client");
    let err = client.floor_price("NACHO").await.expect_err("should error");
    assert!(matches!(err, QuoteError::Status(_)), "got {err:?}");
}

#[tokio::test]
async fn http_client_maps_malformed_body() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_raw("garbage", "text/plain"))
        .mount(&server)
        .await;

    let client = KaspaComFloorPrice::new(server.uri(), Duration::from_secs(2)).expect("client");
    let err = client.floor_price("NACHO").await.expect_err("should error");
    assert!(matches!(err, QuoteError::Malformed(_)), "got {err:?}");
}
