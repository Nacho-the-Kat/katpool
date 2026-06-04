//! KAS payout dry-run rehearsal — evidence assembly.
//!
//! The binary ([`main`](../katpool_payout_rehearsal/index.html)) drives one
//! dry-run payout cycle through the production engine
//! ([`payout_kas::PayoutEngine`] with [`payout_kas::ExecutionMode::DryRun`]):
//! it plans + signs + verifies against the live treasury UTXO set but never
//! broadcasts and never marks rows submitted. The planned cycle, the planned
//! `payout` rows, and the `cycle.plan` / `cycle.reconcile` audit trail are the
//! evidence.
//!
//! This library holds the pure, deterministic part — [`RehearsalEvidence`] and
//! its [`RehearsalEvidence::to_envelope`] serializer — so the exact JSON
//! contract the operator captures is unit-tested without a database or node.

#![cfg_attr(not(test), warn(missing_docs))]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
    )
)]

use katpool_db::repo::audit::AuditLogEntry;
use katpool_db::repo::payout::{
    KasEligibleWallet, Payout, PayoutCycle, PayoutCycleStatus, PayoutKind, PayoutStatus,
};
use payout_kas::{ConfirmReport, ExecutionReport};
use serde_json::{Value, json};

/// Crate version constant.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Stable schema identifier for the reconcile envelope.
pub const ENVELOPE_SCHEMA: &str = "katpool-payout-rehearsal.reconcile/v1";

/// Parameters the rehearsal ran with (echoed into the envelope for provenance).
#[derive(Debug, Clone)]
pub struct RehearsalParams {
    /// Instance label used for the engine and advisory lock.
    pub instance_id: String,
    /// Schema-network identifier (`mainnet`, `testnet-10`, …).
    pub network: String,
    /// Treasury (pool) address funds would come from.
    pub treasury_address: String,
    /// Eligibility threshold in sompi.
    pub threshold_sompi: i64,
    /// DAA width of the payout cycle window.
    pub cycle_span_daa: u64,
    /// Virtual DAA score observed at plan time.
    pub virtual_daa: u64,
}

/// Everything gathered from one dry-run cycle, ready to serialize.
pub struct RehearsalEvidence<'a> {
    /// The parameters the rehearsal ran with.
    pub params: &'a RehearsalParams,
    /// Pre-plan snapshot of payable wallets at the threshold.
    pub eligible: &'a [KasEligibleWallet],
    /// The planned cycle row.
    pub cycle: &'a PayoutCycle,
    /// The planned per-recipient payout rows.
    pub payouts: &'a [Payout],
    /// Dry-run broadcast report (signed + verified, nothing sent).
    pub broadcast: &'a ExecutionReport,
    /// Confirmation report (expected empty for a fresh dry-run).
    pub confirm: &'a ConfirmReport,
    /// Cycle status after reconcile.
    pub reconciled_status: PayoutCycleStatus,
    /// Audit entries for this cycle (`cycle.plan`, `cycle.reconcile`, …).
    pub audit: &'a [AuditLogEntry],
}

impl RehearsalEvidence<'_> {
    /// Assemble the canonical reconcile JSON envelope.
    #[must_use]
    pub fn to_envelope(&self) -> Value {
        let total_payable = self
            .eligible
            .iter()
            .fold(0_i64, |acc, w| acc.saturating_add(w.payable_sompi));

        let counts = payout_status_histogram(self.payouts);

        json!({
            "schema": ENVELOPE_SCHEMA,
            "version": VERSION,
            "dry_run": true,
            "instance_id": self.params.instance_id,
            "network": self.params.network,
            "treasury_address": self.params.treasury_address,
            "params": {
                "threshold_sompi": self.params.threshold_sompi,
                "cycle_span_daa": self.params.cycle_span_daa,
                "virtual_daa": self.params.virtual_daa,
            },
            "eligible_wallets": {
                "count": self.eligible.len(),
                "total_payable_sompi": total_payable,
                "wallets": self.eligible.iter().map(eligible_to_json).collect::<Vec<_>>(),
            },
            "cycle": cycle_to_json(self.cycle),
            "payouts": self.payouts.iter().map(payout_to_json).collect::<Vec<_>>(),
            "counts": {
                "total": self.payouts.len(),
                "planned": counts.planned,
                "submitted": counts.submitted,
                "accepted": counts.accepted,
                "confirmed": counts.confirmed,
                "failed": counts.failed,
            },
            "broadcast": execution_report_to_json(self.broadcast),
            "confirm": {
                "accepted": self.confirm.accepted,
                "confirmed": self.confirm.confirmed,
                "pending": self.confirm.pending,
                "unknown": self.confirm.unknown,
            },
            "reconciled_status": cycle_status_str(self.reconciled_status),
            "audit": self.audit.iter().map(audit_to_json).collect::<Vec<_>>(),
        })
    }
}

#[derive(Default)]
struct StatusHistogram {
    planned: usize,
    submitted: usize,
    accepted: usize,
    confirmed: usize,
    failed: usize,
}

fn payout_status_histogram(payouts: &[Payout]) -> StatusHistogram {
    let mut h = StatusHistogram::default();
    for p in payouts {
        match p.status {
            PayoutStatus::Planned => h.planned += 1,
            PayoutStatus::Submitted => h.submitted += 1,
            PayoutStatus::Accepted => h.accepted += 1,
            PayoutStatus::Confirmed => h.confirmed += 1,
            PayoutStatus::Failed => h.failed += 1,
        }
    }
    h
}

fn eligible_to_json(w: &KasEligibleWallet) -> Value {
    json!({
        "wallet_id": w.wallet_id.0,
        "address": w.address,
        "network": w.network,
        "allocated_sompi": w.allocated_sompi,
        "confirmed_paid_sompi": w.confirmed_paid_sompi,
        "payable_sompi": w.payable_sompi,
    })
}

fn cycle_to_json(c: &PayoutCycle) -> Value {
    json!({
        "id": c.id,
        "kind": payout_kind_str(c.kind),
        "status": cycle_status_str(c.status),
        "daa_start": c.daa_start,
        "daa_end": c.daa_end,
        "total_sompi": c.total_sompi,
        "total_recipients": c.total_recipients,
        "idempotency_key": c.idempotency_key,
        "planned_at": c.planned_at.to_rfc3339(),
        "broadcast_at": c.broadcast_at.map(|t| t.to_rfc3339()),
        "settled_at": c.settled_at.map(|t| t.to_rfc3339()),
    })
}

fn payout_to_json(p: &Payout) -> Value {
    json!({
        "id": p.id,
        "wallet_id": p.wallet_id.0,
        "amount_sompi": p.amount_sompi,
        "status": payout_status_str(p.status),
        "tx_hash": p.tx_hash.as_deref().map(hex_str),
        "planned_at": p.planned_at.to_rfc3339(),
        "submitted_at": p.submitted_at.map(|t| t.to_rfc3339()),
        "confirmed_at": p.confirmed_at.map(|t| t.to_rfc3339()),
        "failure_reason": p.failure_reason,
    })
}

fn execution_report_to_json(r: &ExecutionReport) -> Value {
    json!({
        "planned_batches": r.planned_batches,
        "spendable_utxos": r.spendable_utxos,
        "submitted_txids": r.submitted_txids.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "submitted_payouts": r.submitted_payouts,
        "deferred_below_floor": r.deferred_below_floor,
        "unpaid": r.unpaid,
        "submit_errors": r.submit_errors,
    })
}

fn audit_to_json(e: &AuditLogEntry) -> Value {
    json!({
        "id": e.id.0,
        "occurred_at": e.occurred_at.to_rfc3339(),
        "actor": e.actor,
        "action": e.action,
        "subject_type": e.subject_type,
        "subject_id": e.subject_id,
        "correlation_id": e.correlation_id.map(|id| id.to_string()),
        "payload": e.payload,
    })
}

fn hex_str(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

const fn cycle_status_str(s: PayoutCycleStatus) -> &'static str {
    match s {
        PayoutCycleStatus::Planned => "planned",
        PayoutCycleStatus::Broadcasting => "broadcasting",
        PayoutCycleStatus::PartiallySettled => "partially_settled",
        PayoutCycleStatus::Settled => "settled",
        PayoutCycleStatus::Failed => "failed",
    }
}

const fn payout_status_str(s: PayoutStatus) -> &'static str {
    match s {
        PayoutStatus::Planned => "planned",
        PayoutStatus::Submitted => "submitted",
        PayoutStatus::Accepted => "accepted",
        PayoutStatus::Confirmed => "confirmed",
        PayoutStatus::Failed => "failed",
    }
}

const fn payout_kind_str(k: PayoutKind) -> &'static str {
    match k {
        PayoutKind::Kas => "kas",
        PayoutKind::Krc20Nacho => "krc20_nacho",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use katpool_db::repo::WalletId;
    use katpool_db::repo::audit::AuditLogEntry;
    use katpool_db::repo::payout::PayoutKind;

    fn sample_cycle() -> PayoutCycle {
        PayoutCycle {
            id: 42,
            kind: PayoutKind::Kas,
            status: PayoutCycleStatus::Planned,
            daa_start: 0,
            daa_end: 86_400,
            planned_at: Utc.with_ymd_and_hms(2026, 5, 31, 12, 0, 0).unwrap(),
            broadcast_at: None,
            settled_at: None,
            total_sompi: 2_000_000_000,
            total_recipients: 1,
            idempotency_key: "kas-0-86400".to_owned(),
        }
    }

    fn sample_payout() -> Payout {
        Payout {
            id: 7,
            cycle_id: 42,
            wallet_id: WalletId(3),
            amount_sompi: 2_000_000_000,
            status: PayoutStatus::Planned,
            tx_hash: Some(vec![0xde, 0xad, 0xbe, 0xef]),
            krc20_commit_hash: None,
            krc20_reveal_hash: None,
            planned_at: Utc.with_ymd_and_hms(2026, 5, 31, 12, 0, 1).unwrap(),
            submitted_at: None,
            confirmed_at: None,
            accepted_daa_score: None,
            failure_reason: None,
        }
    }

    fn sample_audit() -> AuditLogEntry {
        AuditLogEntry {
            id: katpool_db::repo::AuditLogId(99),
            occurred_at: Utc.with_ymd_and_hms(2026, 5, 31, 12, 0, 2).unwrap(),
            actor: "payout-kas".to_owned(),
            action: "cycle.plan".to_owned(),
            subject_type: Some("payout_cycle".to_owned()),
            subject_id: Some(42),
            correlation_id: None,
            payload: json!({"recipients": 1}),
        }
    }

    #[test]
    fn envelope_has_stable_schema_and_dry_run_contract() {
        let params = RehearsalParams {
            instance_id: "katpool-rehearsal".to_owned(),
            network: "testnet-10".to_owned(),
            treasury_address: "kaspatest:qexample".to_owned(),
            threshold_sompi: 500_000_000,
            cycle_span_daa: 86_400,
            virtual_daa: 12_345,
        };
        let cycle = sample_cycle();
        let payouts = vec![sample_payout()];
        let broadcast = ExecutionReport {
            planned_batches: 1,
            ..ExecutionReport::default()
        };
        let confirm = ConfirmReport::default();
        let audit = vec![sample_audit()];

        let evidence = RehearsalEvidence {
            params: &params,
            eligible: &[],
            cycle: &cycle,
            payouts: &payouts,
            broadcast: &broadcast,
            confirm: &confirm,
            reconciled_status: PayoutCycleStatus::Planned,
            audit: &audit,
        };
        let env = evidence.to_envelope();

        assert_eq!(env["schema"], ENVELOPE_SCHEMA);
        assert_eq!(env["dry_run"], true);
        assert_eq!(env["network"], "testnet-10");
        assert_eq!(env["cycle"]["id"], 42);
        assert_eq!(env["cycle"]["idempotency_key"], "kas-0-86400");
        assert_eq!(env["cycle"]["kind"], "kas");
        assert_eq!(env["counts"]["total"], 1);
        assert_eq!(env["counts"]["planned"], 1);
        assert_eq!(env["payouts"][0]["wallet_id"], 3);
        // Kaspa hashes are lowercase hex; tx_hash bytes must round-trip.
        assert_eq!(env["payouts"][0]["tx_hash"], "deadbeef");
        assert_eq!(env["payouts"][0]["status"], "planned");
        assert_eq!(env["broadcast"]["planned_batches"], 1);
        assert_eq!(env["reconciled_status"], "planned");
        assert_eq!(env["audit"][0]["action"], "cycle.plan");
        assert_eq!(env["audit"][0]["subject_type"], "payout_cycle");
    }
}
