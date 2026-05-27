//! Adapter: legacy `katpool-app` monitoring logs → [`PoolEvent`] stream.
//!
//! The legacy stack logs share outcomes to stdout in a stable DEBUG
//! format when `DEBUG=true` (production runs with this enabled).
//! Block lifecycle events (`Block found!`, `Block submission
//! successful`) are emitted only to Datadog structured logs, **not**
//! the monitoring stream — so this adapter covers share + reject
//! ingestion deterministically; block rows require NDJSON capture
//! from the new `katpool` runtime (`KATPOOL_EVENT_RECORD_PATH`) or
//! the `block_details` legacy DB table at cutover.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::Path;
use anyhow::Context;
use chrono::{DateTime, NaiveDateTime, Utc};
use katpool_domain::{
    CorrelationId, DaaScore, PoolEvent, ShareDifficulty, ShareRejectReason, WalletAddress,
    WorkerName,
};
use regex::{Captures, Regex};
use uuid::Uuid;

/// Default pool difficulty on legacy stratum ports (`config.json`).
const LEGACY_DEFAULT_DIFFICULTY: f64 = 2048.0;

/// Outcome of parsing a legacy monitoring log.
#[derive(Debug, Clone)]
pub struct LegacyParseReport {
    /// Deterministic [`PoolEvent`] stream in log order.
    pub events: Vec<PoolEvent>,
    /// Parse counters for operator evidence.
    pub stats: LegacyParseStats,
}

/// Per-line parse counters (for operator evidence).
#[derive(Debug, Clone, Default)]
pub struct LegacyParseStats {
    /// Total lines read from the file.
    pub lines_read: u64,
    /// Events appended to the output stream.
    pub events_emitted: u64,
    /// `ShareCredited` events parsed.
    pub share_credited: u64,
    /// `ShareRejected` events parsed.
    pub share_rejected: u64,
    /// Lines that did not match any known pattern.
    pub lines_unmatched: u64,
    /// Lines where the leading timestamp could not be parsed.
    pub timestamp_parse_failures: u64,
}

/// Parse a legacy monitoring log file into a deterministic event stream.
pub fn parse_legacy_log_path(path: &Path) -> anyhow::Result<LegacyParseReport> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("opening legacy log `{}`", path.display()))?;
    parse_legacy_log_reader(BufReader::new(file))
}

/// Parse legacy monitoring log bytes.
pub fn parse_legacy_log_reader<R: BufRead>(reader: R) -> anyhow::Result<LegacyParseReport> {
    let share_added =
        Regex::new(r"SharesManager \d+: Share added for (\S+) - Address: (\S+) - Nonce: (\d+)")
            .context("compiling share_added regex")?;
    let invalid_share = Regex::new(r"Invalid share for target: \d+ for miner (\S+)")
        .context("compiling invalid_share regex")?;
    let stale_share = Regex::new(r"Stale header for miner (\S+) and hash: (\S+)")
        .context("compiling stale_share regex")?;

    let mut worker_wallet: HashMap<String, String> = HashMap::new();
    let mut events = Vec::new();
    let mut stats = LegacyParseStats::default();

    for (line_no, line) in reader.lines().enumerate() {
        let line = line.with_context(|| format!("reading legacy log line {}", line_no + 1))?;
        stats.lines_read += 1;
        let seq = u64::try_from(line_no + 1)
            .with_context(|| format!("line number overflow at {}", line_no + 1))?;
        let correlation_id = correlation_id_for_sequence(seq);

        let Some((ts, rest)) = split_legacy_timestamp(&line) else {
            stats.timestamp_parse_failures += 1;
            stats.lines_unmatched += 1;
            continue;
        };

        if let Some(caps) = share_added.captures(rest) {
            parse_share_added(&caps, &mut worker_wallet, seq, ts, correlation_id, &mut events, &mut stats)?;
            continue;
        }

        if let Some(caps) = invalid_share.captures(rest) {
            let Some(worker_name) = capture_str(&caps, 1) else {
                stats.lines_unmatched += 1;
                continue;
            };
            emit_reject(
                &worker_wallet,
                worker_name,
                ShareRejectReason::LowDifficulty,
                ts,
                correlation_id,
                &mut events,
                &mut stats,
            )?;
            continue;
        }

        if let Some(caps) = stale_share.captures(rest) {
            let Some(worker_name) = capture_str(&caps, 1) else {
                stats.lines_unmatched += 1;
                continue;
            };
            emit_reject(
                &worker_wallet,
                worker_name,
                ShareRejectReason::Stale,
                ts,
                correlation_id,
                &mut events,
                &mut stats,
            )?;
            continue;
        }

        stats.lines_unmatched += 1;
    }

    Ok(LegacyParseReport { events, stats })
}

fn parse_share_added(
    caps: &Captures<'_>,
    worker_wallet: &mut HashMap<String, String>,
    seq: u64,
    ts: DateTime<Utc>,
    correlation_id: CorrelationId,
    events: &mut Vec<PoolEvent>,
    stats: &mut LegacyParseStats,
) -> anyhow::Result<()> {
    let Some(worker_name) = capture_str(caps, 1) else {
        stats.lines_unmatched += 1;
        return Ok(());
    };
    let Some(address) = capture_str(caps, 2) else {
        stats.lines_unmatched += 1;
        return Ok(());
    };
    worker_wallet.insert(worker_name.to_owned(), address.to_owned());
    let wallet = WalletAddress::new(address.to_owned()).context("share_added wallet")?;
    let worker = WorkerName::new(worker_name.to_owned()).context("share_added worker")?;
    events.push(PoolEvent::ShareCredited {
        wallet,
        worker,
        difficulty: ShareDifficulty::new(LEGACY_DEFAULT_DIFFICULTY)
            .context("legacy default difficulty")?,
        daa_score: DaaScore::new(seq),
        ts,
        correlation_id,
    });
    stats.share_credited += 1;
    stats.events_emitted += 1;
    Ok(())
}

fn emit_reject(
    worker_wallet: &HashMap<String, String>,
    worker_name: &str,
    reason: ShareRejectReason,
    ts: DateTime<Utc>,
    correlation_id: CorrelationId,
    events: &mut Vec<PoolEvent>,
    stats: &mut LegacyParseStats,
) -> anyhow::Result<()> {
    let Some((wallet, worker)) = resolve_worker(worker_wallet, worker_name)? else {
        stats.lines_unmatched += 1;
        return Ok(());
    };
    events.push(PoolEvent::ShareRejected {
        wallet,
        worker,
        reason,
        ts,
        correlation_id,
    });
    stats.share_rejected += 1;
    stats.events_emitted += 1;
    Ok(())
}

fn capture_str<'a>(caps: &'a Captures<'a>, index: usize) -> Option<&'a str> {
    caps.get(index).map(|m| m.as_str())
}

/// Keep every `nth` event (1 = keep all). Used for 1:50 CI subsampling.
#[must_use]
pub fn subsample_every_nth(events: Vec<PoolEvent>, nth: u64) -> Vec<PoolEvent> {
    if nth <= 1 {
        return events;
    }
    events
        .into_iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let one_based = i + 1;
            u64::try_from(one_based)
                .ok()
                .filter(|n| n.is_multiple_of(nth))
                .map(|_| e)
        })
        .collect()
}

fn resolve_worker(
    cache: &HashMap<String, String>,
    worker_name: &str,
) -> anyhow::Result<Option<(WalletAddress, WorkerName)>> {
    let Some(address) = cache.get(worker_name) else {
        return Ok(None);
    };
    Ok(Some((
        WalletAddress::new(address.clone()).context("wallet parse")?,
        WorkerName::new(worker_name.to_owned()).context("worker parse")?,
    )))
}

/// `20-May-2026 18:45:22 INFO: ...` → UTC timestamp + message suffix.
fn split_legacy_timestamp(line: &str) -> Option<(DateTime<Utc>, &str)> {
    let (prefix, rest) = line.split_once(" INFO: ").or_else(|| line.split_once(" DEBUG: "))?;
    let ts = NaiveDateTime::parse_from_str(prefix.trim(), "%d-%b-%Y %H:%M:%S").ok()?;
    Some((DateTime::from_naive_utc_and_offset(ts, Utc), rest))
}

/// Deterministic correlation id from a 1-based sequence number.
#[must_use]
pub fn correlation_id_for_sequence(seq: u64) -> CorrelationId {
    let mut bytes = [0u8; 16];
    bytes[0..8].copy_from_slice(&seq.to_be_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    CorrelationId::from_uuid(Uuid::from_bytes(bytes))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn parses_share_and_reject_lines() {
        let log = "\
20-May-2026 18:45:22 DEBUG: SharesManager 6666: Share added for rig1 - Address: kaspa:qypczcz0lhyf3tfsuqj86e7qc8us7r8a53nhlr4u6x4kq38td0hsjycf7sya7zq - Nonce: 1
20-May-2026 18:45:23 DEBUG: SharesManager 1111: Invalid share for target: 99 for miner rig1
";
        let report = parse_legacy_log_reader(log.as_bytes()).unwrap();
        assert_eq!(report.stats.share_credited, 1);
        assert_eq!(report.stats.share_rejected, 1);
        assert_eq!(report.events.len(), 2);
    }

    #[test]
    fn subsample_keeps_every_nth() {
        let wallet = WalletAddress::new(
            "kaspa:qypczcz0lhyf3tfsuqj86e7qc8us7r8a53nhlr4u6x4kq38td0hsjycf7sya7zq",
        )
        .unwrap();
        let worker = WorkerName::new("w").unwrap();
        let mut events = Vec::new();
        for i in 0..10_u64 {
            events.push(PoolEvent::ShareCredited {
                wallet: wallet.clone(),
                worker: worker.clone(),
                difficulty: ShareDifficulty::new(1.0).unwrap(),
                daa_score: DaaScore::new(i),
                ts: Utc::now(),
                correlation_id: CorrelationId::new_v4(),
            });
        }
        let sub = subsample_every_nth(events, 2);
        assert_eq!(sub.len(), 5);
    }
}
